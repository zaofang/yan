import Std.Data.HashMap
import YongK.Syntax

/-! An executable semantics for Yong-K (phase 0 of the formal core plan).

It states the meaning the spec gives, not the Rust implementation's:
- `inout` is copy-in, copy-out (mutable value semantics, 4.1): the callee works on its own value
  and the caller's variable takes the final value when the call returns, including on failure.
- `par` runs its branches in source order (the `sequential` handler, 9.1); the lowest failing
  branch wins (9.2) and later branches do not run.
- panics are immediate (8.3); R380 permits an implementation to raise them later.
These definitions are `partial`: the proofs of later phases use a separate inductive relation that
this interpreter is tested against. -/

namespace YongK

inductive Value where
  | int (n : Int)
  | float (f : Float)
  | bool (b : Bool)
  | str (s : String)
  | unit
  | data (name : String) (fields : List (String × Value))
  | tuple (items : List Value)
  | cap (name : String)
  deriving Inhabited

/-- Abrupt completion of a statement. -/
inductive Sig where
  | ret (v : Value)
  | fail (v : Value)
  | panic (kind : String)
  | brk (label : String)
  | cont (label : String)
  | jump (name : String) (args : List Value)
  | become (callee : String) (args : List Value)
  | unsupported (what : String)
  deriving Inhabited

structure St where
  env : Std.HashMap String Value := {}
  out : Array String := #[]

abbrev M := ExceptT Sig (StateM St)

structure Prog where
  fns : Std.HashMap String Fn
  shapes : Std.HashMap String (List String)

def unsupported {α} (what : String) : M α := throw (.unsupported what)
def panic {α} (kind : String) : M α := throw (.panic kind)

/-- Integer types and their ranges (3.2). -/
def intRange : String → Option (Int × Int)
  | "i8" => some (-2^7, 2^7 - 1) | "i16" => some (-2^15, 2^15 - 1)
  | "i32" => some (-2^31, 2^31 - 1) | "i64" => some (-2^63, 2^63 - 1)
  | "i128" => some (-2^127, 2^127 - 1)
  | "u8" => some (0, 2^8 - 1) | "u16" => some (0, 2^16 - 1) | "u32" => some (0, 2^32 - 1)
  | "u64" => some (0, 2^64 - 1) | "u128" => some (0, 2^128 - 1)
  | _ => none

/-- `a / b` as the correctly rounded f64 (3.4, SQ21). Exact quotient bits plus a sticky bit,
then one rounding by the hardware conversion. -/
def divRound (a b : Int) : Float :=
  if a == 0 then 0 else
  let neg := decide (a < 0) != decide (b < 0)
  let n := a.natAbs
  let d := b.natAbs
  -- pick k with 2^63 <= n * 2^k / d < 2^64, so the quotient has exactly 64 bits
  let quo := fun (k : Int) => if k >= 0 then (n * 2 ^ k.toNat, d) else (n, d * 2 ^ k.natAbs)
  let k0 : Int := 63 - ((Nat.log2 n : Int) - (Nat.log2 d : Int))
  let k := match quo k0 with
    | (x, y) => if x / y >= 2 ^ 64 then k0 - 1 else if x / y < 2 ^ 63 then k0 + 1 else k0
  let (x, y) := quo k
  let q := if x % y == 0 then x / y else (x / y) ||| 1
  let f := Float.scaleB (UInt64.toFloat q.toUInt64) (-k)
  if neg then -f else f

def floorDiv (a b : Int) : Int := Int.fdiv a b
def floorMod (a b : Int) : Int := Int.fmod a b

def intrinsic (name : String) (args : List Value) : Option (M Value) :=
  if name == "exclusive_check" then
    some (match args with
      | [.int a, .int b] => if a == b then panic "exclusivity" else pure .unit
      | _ => unsupported "exclusive_check on non-integers")
  else
  match name.splitOn "_" |>.reverse with
  | [] => none
  | ty :: rest =>
    let op := "_".intercalate rest.reverse
    match intRange ty, args with
    | some (lo, hi), [.int a, .int b] =>
      let checked := fun (r : Int) => if lo <= r && r <= hi then pure (Value.int r) else panic "overflow"
      match op with
      | "add_checked" => some (checked (a + b))
      | "sub_checked" => some (checked (a - b))
      | "mul_checked" => some (checked (a * b))
      | "floordiv" => some (if b == 0 then panic "div_zero" else checked (floorDiv a b))
      | "mod" => some (if b == 0 then panic "div_zero" else checked (floorMod a b))
      | "div" => some (if b == 0 then panic "div_zero" else pure (.float (divRound a b)))
      | "eq" => some (pure (.bool (a == b))) | "ne" => some (pure (.bool (a != b)))
      | "lt" => some (pure (.bool (a < b))) | "le" => some (pure (.bool (a <= b)))
      | "gt" => some (pure (.bool (a > b))) | "ge" => some (pure (.bool (a >= b)))
      | _ => none
    | none, [.float a, .float b] =>
      if ty != "f64" then none else
      match op with
      | "add" => some (pure (.float (a + b))) | "sub" => some (pure (.float (a - b)))
      | "mul" => some (pure (.float (a * b))) | "div" => some (pure (.float (a / b)))
      | "eq" => some (pure (.bool (a == b))) | "ne" => some (pure (.bool (a != b)))
      | "lt" => some (pure (.bool (a < b))) | "le" => some (pure (.bool (a <= b)))
      | "gt" => some (pure (.bool (a > b))) | "ge" => some (pure (.bool (a >= b)))
      | _ => none
    | none, [.bool a, .bool b] =>
      if ty != "bool" then none else
      match op with
      | "and" => some (pure (.bool (a && b))) | "or" => some (pure (.bool (a || b)))
      | "eq" => some (pure (.bool (a == b))) | "ne" => some (pure (.bool (a != b)))
      | _ => none
    | none, [.bool a] => if name == "not_bool" then some (pure (.bool !a)) else none
    | _, _ => none

def litValue : Lit → Value
  | .int n => .int n | .float f => .float f | .bool b => .bool b | .str s => .str s | .unit => .unit

def litEq : Lit → Value → Bool
  | .int n, .int m => n == m
  | .bool a, .bool b => a == b
  | .str a, .str b => a == b
  | .unit, .unit => true
  | .float a, .float b => a == b
  | _, _ => false

def getVar (x : String) : M Value := do
  match (← get).env[x]? with
  | some v => return v
  | none => throw (.unsupported s!"unbound `{x}`")

def setVar (x : String) (v : Value) : M Unit := modify fun s => { s with env := s.env.insert x v }

partial def readPath : Value → List Step → M Value
  | v, [] => pure v
  | .data _ fs, .field f :: rest =>
    match fs.lookup f with
    | some v => readPath v rest
    | none => unsupported s!"no field `{f}`"
  | _, .index _ :: _ => unsupported "indexing"
  | _, _ => unsupported "projection of a non-record"

partial def writePath : Value → List Step → Value → M Value
  | _, [], new => pure new
  | .data n fs, .field f :: rest, new => do
    match fs.lookup f with
    | some v =>
      let v' ← writePath v rest new
      return .data n (fs.map fun (g, w) => if g == f then (g, v') else (g, w))
    | none => unsupported s!"no field `{f}`"
  | _, _, _ => unsupported "assignment through a non-record"

def atomValue (p : Prog) : Atom → M Value
  | .lit l => pure (litValue l)
  | .var x => do
    match (← get).env[x]? with
    | some v => return v
    | none =>
      -- a nullary constructor written as a bare name
      if p.shapes[x]? == some [] then return .data x [] else unsupported s!"unbound `{x}`"

/-- Match `v` against `pat`, returning the bindings. A bare name that is a nullary variant is
that variant, not a binding. -/
partial def matchPat (p : Prog) : Pat → Value → Option (List (String × Value))
  | .wild, _ => some []
  | .lit l, v => if litEq l v then some [] else none
  | .bind x, v =>
    if p.shapes[x]? == some [] then
      match v with | .data n _ => if n == x then some [] else none | _ => none
    else some [(x, v)]
  | .ctor n fs _, .data m vs =>
    if n != m then none else
    fs.foldlM (init := []) fun acc (f, q) => do
      let v ← vs.lookup f
      return acc ++ (← matchPat p q v)
  | .tuple qs, .tuple vs =>
    if qs.length != vs.length then none else
    (qs.zip vs).foldlM (init := []) fun acc (q, v) => do return acc ++ (← matchPat p q v)
  | .alt a b, v => (matchPat p a v).orElse fun _ => matchPat p b v
  | _, _ => none

def bindAll (bs : List (String × Value)) : M Unit := bs.forM fun (x, v) => setVar x v

def asBool (v : Value) : M Bool :=
  match v with | .bool b => pure b | _ => unsupported "condition is not a bool"

mutual

partial def evalArgs (p : Prog) (args : List Arg) : M (List Value) :=
  args.mapM fun
    | .val _ a => atomValue p a
    | .cap n => getVar n

/-- Call `f` with argument values. A `become` in its body continues the loop with the new
callee and arguments in constant stack (R260). Returns the result and the final values of the
`inout` parameters, by position. -/
partial def invoke (p : Prog) (f0 : Fn) (args0 : List Value) : M (Value × List (Option Value)) := do
  let saved := (← get).env
  let mut f := f0
  let mut args := args0
  let mut result : Value := .unit
  repeat
    modify fun s => { s with env := {} }
    bindAll (f.params.map (·.name) |>.zip args)
    for r in f.requires do
      if !(← asBool (← evalRhs p r)) then panic "contract"
    let sig ← tryCatch (do execBlock p f.body; pure none) fun s => pure (some s)
    match sig with
    | some (.become g gargs) =>
      match p.fns[g]? with
      | some g' => f := g'; args := gargs
      | none => modify (fun s => { s with env := saved }); unsupported s!"become to `{g}`"
    | some (.ret v) => result := v; break
    | none => result := .unit; break
    | some s =>
      -- a failure still writes back `inout` parameters
      let outs ← f.params.mapM fun q => if q.conv == .inout then some <$> getVar q.name else pure none
      modify fun st => { st with env := saved }
      match s with
      | .fail v => throw (.fail (.data "__fail" [("value", v), ("outs", .tuple (outs.map (·.getD .unit)))]))
      | s => throw s
  setVar "out" result
  for e in f.ensures do
    if !(← asBool (← evalRhs p e)) then panic "contract"
  let outs ← f.params.mapM fun q => if q.conv == .inout then some <$> getVar q.name else pure none
  modify fun s => { s with env := saved }
  return (result, outs)

partial def writeBack (args : List Arg) (outs : List (Option Value)) : M Unit :=
  (args.zip outs).forM fun
    | (.val .inout (.var x), some v) => setVar x v
    | _ => pure ()

partial def callFn (p : Prog) (c : Call) : M Value := do
  let vs ← evalArgs p c.args
  if c.callee == "std.io.print" then
    match vs with
    | [.cap _, .str s] => modify fun st => { st with out := st.out.push s }; return .unit
    | _ => unsupported "std.io.print arguments"
  if let some r := intrinsic c.callee vs then return ← r
  match p.fns[c.callee]? with
  | none => unsupported s!"call to `{c.callee}`"
  | some f =>
    let r ← tryCatch (some <$> invoke p f vs) fun
      | .fail (.data "__fail" [("value", v), ("outs", .tuple outs)]) => do
        writeBack c.args (f.params.zip outs |>.map fun (q, o) => if q.conv == .inout then some o else none)
        throw (.fail v)
      | s => throw s
    match r with
    | some (v, outs) => writeBack c.args outs; return v
    | none => return .unit

partial def evalRhs (p : Prog) : Rhs → M Value
  | .atom a => atomValue p a
  | .call c => callFn p c
  | .prop c => callFn p c
  | .try_ c => tryCatch (do return .data "Ok" [("value", ← callFn p c)]) fun
      | .fail e => return .data "Err" [("error", e)]
      | s => throw s
  | .ctor n fs => do return .data n (← fs.mapM fun (f, a) => do return (f, ← atomValue p a))
  | .tuple xs => do return .tuple (← xs.mapM (atomValue p))
  | .proj pl | .copy pl | .move pl => do readPath (← getVar pl.root) pl.path

partial def execBlock (p : Prog) (b : Block) : M Unit := do
  let jps := b.filterMap fun | .jp n ps body => some (n, (ps, body)) | _ => none
  for s in b do
    if s matches .jp .. then continue
    tryCatch (execStmt p s) fun
      | .jump k args => runJp p jps k args
      | sig => throw sig

/-- A jump to a join point of this block runs its body, then continues after the statement
that jumped. -/
partial def runJp (p : Prog) (jps : List (String × (List String × Block))) (k : String)
    (args : List Value) : M Unit := do
  match jps.lookup k with
  | none => throw (.jump k args)
  | some (ps, body) =>
    bindAll (ps.zip args)
    tryCatch (execBlock p body) fun
      | .jump k' a' => runJp p jps k' a'
      | sig => throw sig

partial def execStmt (p : Prog) : Stmt → M Unit
  | .let_ _ pat r => do
    let v ← evalRhs p r
    match matchPat p pat v with
    | some bs => bindAll bs
    | none => panic "unreachable"
  | .assign pl r => do
    let v ← evalRhs p r
    let old ← getVar pl.root
    setVar pl.root (← writePath old pl.path v)
  | .if_ c t e => do
    if ← asBool (← atomValue p c) then execBlock p t else execBlock p e
  | .match_ a arms => do
    let v ← atomValue p a
    match arms.findSome? fun (q, b) => (matchPat p q v).map (·, b) with
    | some (bs, b) => bindAll bs; execBlock p b
    | none => panic "unreachable"
  | .loop l body => do
    repeat
      let again ← tryCatch (do execBlock p body; pure true) fun
        | .brk l' => if l' == l then pure false else throw (.brk l')
        | .cont l' => if l' == l then pure true else throw (.cont l')
        | sig => throw sig
      if !again then break
  | .break_ l _ => throw (.brk l)
  | .continue_ l => throw (.cont l)
  | .jp .. => pure ()
  | .jump k args => do throw (.jump k (← args.mapM (atomValue p)))
  | .ret a => do throw (.ret (← atomValue p a))
  | .fail a => do throw (.fail (← atomValue p a))
  | .become c => do
    if (intrinsic c.callee []).isSome || !p.fns.contains c.callee then unsupported "become to a non-function"
    throw (.become c.callee (← evalArgs p c.args))
  | .par bs => bs.forM (execBlock p)
  | .assert r => do if !(← asBool (← evalRhs p r)) then panic "assert"
  | .unreachable => panic "unreachable"
  | .rhs r => do let _ ← evalRhs p r

end

def Prog.ofModule (m : Module) : Prog :=
  { fns := m.fns.foldl (fun h f => h.insert f.name f) {}
    shapes := m.shapes.foldl (fun h s => h.insert s.name s.fields) {} }

/-- How a run ends, as the conformance runner sees it (24.1, 8.3, SQ23). -/
inductive Outcome where
  | ok
  | failed
  | panicked (kind : String)
  | unsupported (what : String)

def runMain (m : Module) : Outcome × Array String :=
  let p := Prog.ofModule m
  match p.fns["main"]? with
  | none => (.unsupported "no `fn main`", #[])
  | some f =>
    let caps := f.params.map fun q => if q.isCap then Value.cap q.name else .unit
    let (r, st) := ((invoke p f caps).run).run {}
    let o := match r with
      | .ok _ => .ok
      | .error (.fail _) => .failed
      | .error (.panic k) => .panicked k
      | .error (.unsupported w) => .unsupported w
      | .error _ => .unsupported "a jump, break or continue escaped its target"
    (o, st.out)

end YongK
