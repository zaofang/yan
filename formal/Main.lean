import YongK

open YongK

def usage : String := "usage: yongk run FILE"

def main (args : List String) : IO UInt32 := do
  match args with
  | ["run", file] =>
    let src ← IO.FS.readFile file
    match parse src with
    | .error e =>
      IO.eprintln s!"yongk: not supported yet: {e}"
      return 2
    | .ok m =>
      let (o, out) := runMain m
      let stdout ← IO.getStdout
      for line in out do stdout.putStrLn line
      match o with
      | .ok => return 0
      | .failed => IO.eprintln "yongk: main failed"; return 1
      | .panicked k => IO.eprintln s!"panic: {k}"; return 101
      | .unsupported w => IO.eprintln s!"yongk: not supported yet: {w}"; return 2
  | _ => IO.eprintln usage; return 2
