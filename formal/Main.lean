import YanCore

open YanCore

def usage : String := "usage: yancore run FILE"

def main (args : List String) : IO UInt32 := do
  match args with
  | ["run", file] =>
    let src ← IO.FS.readFile file
    match parse src with
    | .error e =>
      IO.eprintln s!"yancore: not supported yet: {e}"
      return 2
    | .ok m =>
      -- `yan run` checks first (R901)
      if let bad@(_ :: _) := e602 m then
        for n in bad do IO.eprintln s!"error[E602]: recursive function `{n}` has no `decreases` clause"
        return 1
      let (o, out) := runMain m
      let stdout ← IO.getStdout
      for line in out do stdout.putStrLn line
      match o with
      | .ok => return 0
      | .failed => IO.eprintln "yancore: main failed"; return 1
      | .panicked k => IO.eprintln s!"panic: {k}"; return 101
      | .unsupported w => IO.eprintln s!"yancore: not supported yet: {w}"; return 2
  | _ => IO.eprintln usage; return 2
