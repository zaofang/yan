//! Prints each file named on the command line in the canonical `.yir` form, in the format that
//! `selfhost/s0/core_print.yn` prints, so the two printers can be compared byte for byte (#1505).

use yan_core::{parse, print};

fn main() {
    for p in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&p).expect("readable file");
        println!("file {p}");
        let (module, diags) = parse(&p, &src);
        for d in &diags {
            println!("diag {} {}:{}", d.code, d.span.line, d.span.col);
        }
        if let Some(m) = module {
            println!("{}", print(&m));
        }
    }
}
