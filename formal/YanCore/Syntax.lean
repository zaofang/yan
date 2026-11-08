/-! Abstract syntax of Yan-K, the mechanized fragment of Yan Core text (spec 3.5). -/

namespace YanCore

inductive Conv where
  | let_ | inout | sink
  deriving Repr, BEq, Inhabited

inductive Ty where
  | named (path : String) (args : List Ty)
  deriving Repr, Inhabited

inductive Lit where
  | int (n : Int)
  | float (f : Float)
  | bool (b : Bool)
  | str (s : String)
  | unit
  deriving Repr, Inhabited

inductive Atom where
  | var (name : String)
  | lit (l : Lit)
  deriving Repr, Inhabited

inductive Arg where
  | val (conv : Conv) (a : Atom)
  | cap (name : String)
  deriving Repr, Inhabited

structure Call where
  callee : String
  args : List Arg
  deriving Repr, Inhabited

inductive Step where
  | field (name : String)
  | index (a : Atom)
  deriving Repr, Inhabited

structure Place where
  root : String
  path : List Step
  deriving Repr, Inhabited

inductive Rhs where
  | atom (a : Atom)
  | call (c : Call)
  | prop (c : Call)
  | try_ (c : Call)
  | ctor (name : String) (fields : List (String × Atom))
  | tuple (items : List Atom)
  | proj (p : Place)
  | copy (p : Place)
  | move (p : Place)
  deriving Repr, Inhabited

inductive Pat where
  | wild
  | lit (l : Lit)
  | bind (name : String)
  | ctor (name : String) (fields : List (String × Pat)) (rest : Bool)
  | tuple (items : List Pat)
  | alt (p q : Pat)
  deriving Repr, Inhabited

structure Param where
  conv : Conv
  name : String
  ty : Ty
  isCap : Bool
  deriving Repr, Inhabited

inductive Stmt where
  | let_ (mutable : Bool) (pat : Pat) (rhs : Rhs)
  | assign (place : Place) (rhs : Rhs)
  | if_ (cond : Atom) (thn els : List Stmt)
  | match_ (scrutinee : Atom) (arms : List (Pat × List Stmt))
  | loop (label : String) (body : List Stmt)
  | break_ (label : String) (value : Option Atom)
  | continue_ (label : String)
  | jp (name : String) (params : List String) (body : List Stmt)
  | jump (name : String) (args : List Atom)
  | ret (a : Atom)
  | fail (a : Atom)
  | become (c : Call)
  | par (branches : List (List Stmt))
  | assert (pred : Rhs)
  | unreachable
  | rhs (r : Rhs)
  deriving Repr, Inhabited

abbrev Block := List Stmt

structure Fn where
  name : String
  params : List Param
  requires : List Rhs
  ensures : List Rhs
  body : Block
  /-- has a `decreases` clause (8.4) -/
  decreases : Bool := false
  /-- has an `example` clause or a loop `invariant`; with `requires`/`ensures` these make a module contractual (R940) -/
  contractual : Bool := false
  deriving Repr, Inhabited

/-- A record or a sum variant: its field names in declaration order. -/
structure Shape where
  name : String
  fields : List String
  deriving Repr, Inhabited

structure Module where
  name : String
  fns : List Fn
  shapes : List Shape
  deriving Repr, Inhabited

end YanCore
