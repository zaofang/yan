import YongK.Syntax

/-! A reader for the Yong-K subset of the Yong text grammar (spec 3.5).
It is independent of the Rust parser on purpose: differential testing needs two readings. -/

namespace YongK

inductive Tok where
  | ident (s : String)
  | int (n : Int)
  | float (text : String)
  | str (s : String)
  | sym (s : String)
  deriving Repr, BEq, Inhabited

structure Token where
  tok : Tok
  line : Nat
  deriving Inhabited

private def symbols : List String :=
  ["..=", "->", "=>", "..", "{", "}", "(", ")", "[", "]", ",", ":", "=", ".", "?", "|", "@", "'", "+", "*", "<"]

private def isIdentStart (c : Char) : Bool := c.isAlpha || c == '_'
private def isIdentChar (c : Char) : Bool := c.isAlphanum || c == '_'

def lex (src : String) : Except String (Array Token) := Id.run do
  let cs := src.toList.toArray
  let mut i := 0
  let mut line := 1
  let mut out : Array Token := #[]
  let ch := fun (k : Nat) => cs.getD k '\x00'
  while i < cs.size do
    let c := ch i
    if c == '\n' then
      line := line + 1; i := i + 1
    else if c.isWhitespace then
      i := i + 1
    else if c == '-' && ch (i+1) == '-' then
      while i < cs.size && ch i != '\n' do i := i + 1
    else if c.isDigit || (c == '-' && (ch (i+1)).isDigit) then
      let start := i
      i := i + 1
      while (ch i).isDigit || ch i == '_' do i := i + 1
      let mut isFloat := false
      if ch i == '.' && (ch (i+1)).isDigit then
        isFloat := true
        i := i + 1
        while (ch i).isDigit do i := i + 1
      if (ch i == 'e' || ch i == 'E') && ((ch (i+1)).isDigit || ((ch (i+1) == '-' || ch (i+1) == '+') && (ch (i+2)).isDigit)) then
        isFloat := true
        i := i + 2
        while (ch i).isDigit do i := i + 1
      let text := String.mk ((cs.extract start i).toList.filter (· != '_'))
      if isFloat then
        out := out.push ⟨.float text, line⟩
      else
        let n := if text.startsWith "-" then -((text.drop 1).toNat! : Int) else (text.toNat! : Int)
        out := out.push ⟨.int n, line⟩
    else if isIdentStart c then
      let start := i
      while isIdentChar (ch i) do i := i + 1
      out := out.push ⟨.ident (String.mk (cs.extract start i).toList), line⟩
    else if c == '"' then
      i := i + 1
      let mut s := ""
      while i < cs.size && ch i != '"' do
        if ch i == '\\' then
          let e := ch (i+1)
          s := s.push (match e with | 'n' => '\n' | 't' => '\t' | '0' => '\x00' | x => x)
          i := i + 2
        else
          if ch i == '\n' then line := line + 1
          s := s.push (ch i); i := i + 1
      if i >= cs.size then return .error s!"line {line}: unterminated string"
      i := i + 1
      out := out.push ⟨.str s, line⟩
    else
      match symbols.find? (fun s => (s.toList.zipIdx.all fun (x, k) => ch (i+k) == x)) with
      | some s => out := out.push ⟨.sym s, line⟩; i := i + s.length
      | none => return .error s!"line {line}: unexpected character {c}"
  return .ok out

abbrev P := ReaderT (Array Token) (StateT Nat (Except String))

namespace P

def peekAt (k : Nat) : P (Option Token) := do
  return (← read)[(← get) + k]?

def peek : P (Option Tok) := return (← peekAt 0).map (·.tok)

def fail {α} (msg : String) : P α := do
  let line := ((← peekAt 0).map (·.line)).getD 0
  throw s!"line {line}: {msg}"

def advance : P Unit := modify (· + 1)

def next : P Tok := do
  match ← peek with
  | some t => advance; return t
  | none => fail "unexpected end of file"

def isSym (s : String) : P Bool := return (← peek) == some (.sym s)
def isKw (s : String) : P Bool := return (← peek) == some (.ident s)

def sym (s : String) : P Unit := do
  if ← isSym s then advance else fail s!"expected `{s}`"

def kw (s : String) : P Unit := do
  if ← isKw s then advance else fail s!"expected `{s}`"

def eatSym (s : String) : P Bool := do
  if ← isSym s then advance; return true else return false

def eatKw (s : String) : P Bool := do
  if ← isKw s then advance; return true else return false

def ident : P String := do
  match ← next with
  | .ident s => return s
  | _ => fail "expected a name"

def path : P String := do
  let mut p ← ident
  while (← isSym ".") && ((← peekAt 1).map (·.tok) matches some (Tok.ident _)) do
    advance; p := p ++ "." ++ (← ident)
  return p

/-- Try `p`; on failure rewind and return none. -/
def attempt {α} (p : P α) : P (Option α) := do
  let save ← get
  try some <$> p catch _ => do set save; return none

partial def sepBy {α} (close : String) (p : P α) : P (List α) := do
  let mut xs := #[]
  while !(← isSym close) do
    xs := xs.push (← p)
    if !(← eatSym ",") then break
  sym close
  return xs.toList

/-- Skip a balanced bracket group starting at the current opener. -/
partial def skipGroup : P Unit := do
  let close := match ← next with
    | .sym "(" => ")" | .sym "[" => "]" | .sym "{" => "}" | _ => ""
  while !(← isSym close) do
    match ← peek with
    | some (.sym "(") | some (.sym "[") | some (.sym "{") => skipGroup
    | _ => advance
  advance

end P

open P

def keywords : List String :=
  ["let", "var", "if", "else", "match", "loop", "break", "continue", "jp", "jump", "ret", "fail",
   "become", "par", "assert", "unreachable", "with", "comptime", "unsafe", "asm", "drop", "reuse",
   "yield", "fn", "record", "sum", "module", "try", "copy", "move", "cap", "inout", "sink"]

def literal : P (Option Lit) := do
  match ← peek with
  | some (.int n) => advance; return some (.int n)
  | some (.float t) =>
    advance
    -- digits, optional fraction, optional exponent: an exact decimal, rounded once
    let neg := t.startsWith "-"
    let t := if neg then t.drop 1 else t
    let (mant, expPart) := match t.splitOn "e" with
      | [m, e] => (m, e) | _ => match t.splitOn "E" with | [m, e] => (m, e) | _ => (t, "0")
    let (ip, fp) := match mant.splitOn "." with | [a, b] => (a, b) | _ => (mant, "")
    let digits := (ip ++ fp).toNat!
    let e : Int := (if expPart.startsWith "-" then -((expPart.drop 1).toNat! : Int)
      else ((expPart.dropWhile (· == '+')).toNat! : Int)) - fp.length
    let f := if e >= 0 then Float.ofScientific (digits * 10 ^ e.toNat) false 0
      else Float.ofScientific digits true e.natAbs
    return some (.float (if neg then -f else f))
  | some (.str s) => advance; return some (.str s)
  | some (.ident "true") => advance; return some (.bool true)
  | some (.ident "false") => advance; return some (.bool false)
  | some (.sym "(") =>
    if (← peekAt 1).map (·.tok) == some (.sym ")") then advance; advance; return some .unit
    return none
  | _ => return none

def atom : P Atom := do
  if let some l ← literal then return .lit l
  return .var (← ident)

partial def ty : P Ty := do
  let name ← path
  if ← isSym "[" then
    advance
    let args ← sepBy "]" (do
      match ← peek with
      | some (.int n) => advance; return .named (toString n) []
      | _ => ty)
    return .named name args
  if ← isSym "(" then
    advance
    let args ← sepBy ")" (do
      let _ ← eatKw "let"; let _ ← eatKw "inout"; let _ ← eatKw "sink"; ty)
    if ← eatSym "->" then
      let r ← ty
      if ← eatKw "uses" then
        let _ ← ident
        while ← eatSym "," do let _ ← ident
      if ← eatKw "fail" then let _ ← failTy
      return .named name (args ++ [r])
    return .named name args
  return .named name []
where
  failTy : P Ty := do
    let t ← ty
    if ← eatSym "|" then let _ ← failTy
    return t

def conv : P Conv := do
  if ← eatKw "let" then return .let_
  if ← eatKw "inout" then return .inout
  if ← eatKw "sink" then return .sink
  fail "expected `let`, `inout` or `sink`"

def arg : P Arg := do
  if ← eatKw "cap" then return .cap (← ident)
  let c ← conv
  return .val c (← atom)

def place : P Place := do
  let root ← ident
  let mut steps := #[]
  repeat
    if ← eatSym "." then steps := steps.push (.field (← ident))
    else if ← eatSym "[" then
      steps := steps.push (.index (← atom)); sym "]"
    else break
  return ⟨root, steps.toList⟩

def callTail (callee : String) : P Call := do
  if ← isSym "[" then skipGroup
  sym "("
  return ⟨callee, ← sepBy ")" arg⟩

def call : P Call := do callTail (← path)

def rhs : P Rhs := do
  if ← eatKw "try" then return .try_ (← call)
  if ← eatKw "copy" then return .copy (← place)
  if ← eatKw "move" then return .move (← place)
  if let some l ← literal then return .atom (.lit l)
  if ← isSym "(" then
    advance; return .tuple (← sepBy ")" atom)
  if ← isKw "fn" then fail "not supported yet: closures"
  let save ← get
  let p ← path
  -- `f[T](...)` is a generic call; `xs[i]` is a place
  let isCall ← do
    if ← isSym "(" then pure true
    else if ← isSym "[" then
      let s ← get; skipGroup
      let r ← isSym "("; set s; pure r
    else pure false
  if isCall then
    let c ← callTail p
    if ← eatSym "?" then
      if ← eatKw "via" then let _ ← path
      return .prop c
    return .call c
  if (← isSym "{") && (← do
      let t1 := (← peekAt 1).map (·.tok); let t2 := (← peekAt 2).map (·.tok)
      pure (t1 == some (.sym "}") || (t1 matches some (Tok.ident _)) && t2 == some (.sym ":"))) then
    advance
    let fields ← sepBy "}" (do let f ← ident; sym ":"; return (f, ← atom))
    return .ctor p fields
  if p.contains '.' || (← isSym "[") then
    set save; return .proj (← place)
  return .atom (.var p)

partial def pat : P Pat := do
  let p ← one
  if ← eatSym "|" then return .alt p (← pat)
  return p
where
  one : P Pat := do
    if ← isSym "(" then
      if (← peekAt 1).map (·.tok) != some (.sym ")") then
        advance; return .tuple (← sepBy ")" pat)
    if let some l ← literal then
      if ← isSym "..=" then fail "not supported yet: range patterns"
      return .lit l
    let n ← path
    if n == "_" then return .wild
    if ← eatSym "{" then
      let mut fs := #[]
      let mut rest := false
      while !(← isSym "}") do
        if ← eatSym ".." then rest := true; break
        let f ← ident; sym ":"; fs := fs.push (f, ← pat)
        if !(← eatSym ",") then break
      sym "}"
      return .ctor n fs.toList rest
    return .bind n

def label : P String := do sym "'"; ident

def onSameLine : P Bool := do
  let s ← get; let toks ← read
  match toks[s - 1]?, toks[s]? with
  | some a, some b => return a.line == b.line
  | _, _ => return false

def pred : P Rhs := do
  match ← attempt call with
  | some c => return .call c
  | none => return .atom (← atom)

mutual
partial def block : P Block := do
  sym "{"
  let mut ss := #[]
  while !(← isSym "}") do ss := ss.push (← stmt)
  sym "}"
  return ss.toList

partial def stmt : P Stmt := do
  match ← peek with
  | some (.ident "let") | some (.ident "var") =>
    let m := (← next) == Tok.ident "var"
    let p ← pat; sym ":"; let _ ← ty; sym "="
    return .let_ m p (← rhs)
  | some (.ident "if") =>
    advance
    let c ← atom; let t ← block
    let e ← if ← eatKw "else" then block else pure []
    return .if_ c t e
  | some (.ident "match") =>
    advance
    let s ← atom; sym "{"
    let mut arms := #[]
    while !(← isSym "}") do
      let p ← pat; sym "=>"; arms := arms.push (p, ← block)
    sym "}"
    return .match_ s arms.toList
  | some (.ident "loop") =>
    advance
    let l ← label
    while ← eatKw "invariant" do let _ ← pred
    if ← eatKw "decreases" then
      let _ ← pred
      while ← eatSym "," do let _ ← pred
    return .loop l (← block)
  | some (.ident "break") =>
    advance
    let l ← label
    let v ← if (← onSameLine) && !((← peek) == some (.sym "}")) then some <$> atom else pure none
    return .break_ l v
  | some (.ident "continue") => advance; return .continue_ (← label)
  | some (.ident "jp") =>
    advance
    let n ← ident; sym "("
    let ps ← sepBy ")" (do let _ ← conv; let x ← ident; sym ":"; let _ ← ty; return x)
    return .jp n ps (← block)
  | some (.ident "jump") =>
    advance
    let n ← ident; sym "("
    return .jump n (← sepBy ")" atom)
  | some (.ident "ret") => advance; return .ret (← atom)
  | some (.ident "fail") => advance; return .fail (← atom)
  | some (.ident "become") => advance; return .become (← call)
  | some (.ident "par") =>
    advance
    if ← isKw "concurrent" then fail "not supported yet: par concurrent"
    sym "{"
    let mut bs := #[]
    while !(← isSym "}") do bs := bs.push (← block)
    sym "}"
    return .par bs.toList
  | some (.ident "assert") => advance; return .assert (← pred)
  | some (.ident "unreachable") => advance; return .unreachable
  | some (.ident k) =>
    if ["with", "comptime", "unsafe", "asm", "drop", "reuse", "yield"].contains k then
      fail s!"not supported yet: `{k}`"
    match ← attempt (do let p ← place; sym "="; pure p) with
    | some p => return .assign p (← rhs)
    | none => return .rhs (← rhs)
  | _ => return .rhs (← rhs)
end

def skipAttrs : P Unit := do
  while ← eatSym "@" do
    let _ ← path
    if ← isSym "(" then skipGroup

def param : P Param := do
  let _ ← eatKw "comptime"
  if ← eatKw "cap" then
    let n ← ident; sym ":"
    return ⟨.let_, n, ← ty, true⟩
  let c ← conv
  let n ← ident; sym ":"
  return ⟨c, n, ← ty, false⟩

def fnItem : P Fn := do
  let name ← path
  if ← isSym "[" then skipGroup
  sym "("
  let ps ← sepBy ")" param
  sym "->"
  let _ ← ty
  if ← eatKw "from" then
    let _ ← ident
    while ← eatSym "," do let _ ← ident
  if ← eatKw "fail" then let _ ← ty.failTy
  let mut req := #[]
  let mut ens := #[]
  repeat
    if ← eatKw "requires" then req := req.push (← pred)
    else if ← eatKw "ensures" then ens := ens.push (← pred)
    else if ← eatKw "decreases" then
      let _ ← pred
      while ← eatSym "," do let _ ← pred
    else if ← eatKw "example" then let _ ← pred
    else if ← isKw "cost" then fail "not supported yet: cost"
    else break
  return ⟨name, ps, req.toList, ens.toList, ← block⟩

def fields : P (List String) := do
  sym "{"
  sepBy "}" (do
    let _ ← eatKw "pub"
    let f ← ident; sym ":"; let _ ← ty
    if ← eatSym "=" then let _ ← literal
    return f)

def moduleP : P Module := do
  kw "module"
  let name ← path
  let mut fns := #[]
  let mut shapes := #[]
  while (← peek).isSome do
    skipAttrs
    let _ ← eatKw "pub"
    let _ ← eatKw "unsafe"
    match ← next with
    | .ident "fn" => fns := fns.push (← fnItem)
    | .ident "record" =>
      let n ← path
      if ← isSym "[" then skipGroup
      shapes := shapes.push ⟨n, ← fields⟩
    | .ident "sum" =>
      let _ ← path
      if ← isSym "[" then skipGroup
      sym "{"
      let vs ← sepBy "}" (do
        let v ← ident
        let fs ← if ← isSym "{" then fields else pure []
        return (⟨v, fs⟩ : Shape))
      shapes := shapes ++ vs.toArray
    | .ident k => fail s!"not supported yet: `{k}` items"
    | _ => fail "expected an item"
  return ⟨name, fns.toList, shapes.toList⟩

def parse (src : String) : Except String Module := do
  let toks ← lex src
  let (m, _) ← (moduleP.run toks).run 0
  return m

end YongK
