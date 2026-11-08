import YanCore.Syntax

namespace YanCore

/-- Names a function calls directly, `become` included. -/
partial def callees : Block → List String
  | [] => []
  | s :: rest => stmt s ++ callees rest
where
  rhs : Rhs → List String
    | .call c | .prop c | .try_ c => [c.callee]
    | _ => []
  stmt : Stmt → List String
    | .let_ _ _ r | .assign _ r | .assert r | .rhs r => rhs r
    | .if_ _ t e => callees t ++ callees e
    | .match_ _ arms => arms.foldl (fun a (_, b) => a ++ callees b) []
    | .loop _ b | .jp _ _ b => callees b
    | .become c => [c.callee]
    | .par bs => bs.foldl (fun a b => a ++ callees b) []
    | _ => []

/-- Does a path through the call graph lead from `f` back to `f`? -/
partial def reaches (m : Module) (f : String) : Bool :=
  go (direct f) []
where
  direct (n : String) : List String :=
    match m.fns.find? (·.name == n) with
    | some g => callees g.body
    | none => []
  go : List String → List String → Bool
    | [], _ => false
    | n :: todo, seen =>
      n == f || (if seen.contains n then go todo seen else go (direct n ++ todo) (n :: seen))

/-- E602 (8.4, R940): in a module with a contract clause, a recursive function needs `decreases`.
Returns the offending function names. -/
def e602 (m : Module) : List String :=
  let contracts := m.fns.any fun g => !g.requires.isEmpty || !g.ensures.isEmpty || g.contractual
  if !contracts then [] else
    (m.fns.filter fun g => !g.decreases && reaches m g.name).map (·.name)

end YanCore
