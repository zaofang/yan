//! DWARF line tables and function entries for object emission (T32).

use std::path::Path;

use gimli::write::{
    Address, AttributeValue, DwarfUnit, EndianVec, LineProgram, LineString, RelocateWriter,
    Relocation, RelocationTarget, Sections,
};
use gimli::{Encoding, Format, LineEncoding, RunTimeEndian, constants};
use object::SectionKind;
use object::write::StandardSegment;
use object::write::{Object, Relocation as ObjRelocation, SymbolId};
use object::{BinaryFormat, RelocationEncoding, RelocationFlags, RelocationKind};

/// One compiled function: its symbol, extent and `(offset, line)` rows.
pub struct Func {
    pub symbol: SymbolId,
    pub name: String,
    pub size: u64,
    pub rows: Vec<(u32, u32)>,
}

#[derive(Clone)]
struct Section {
    data: EndianVec<RunTimeEndian>,
    relocs: Vec<Relocation>,
}

impl RelocateWriter for Section {
    type Writer = EndianVec<RunTimeEndian>;

    fn writer(&self) -> &Self::Writer {
        &self.data
    }

    fn writer_mut(&mut self) -> &mut Self::Writer {
        &mut self.data
    }

    fn relocate(&mut self, relocation: Relocation) {
        self.relocs.push(relocation);
    }
}

/// Add `.debug_*` sections for `funcs`, defined in `file`, to `obj`.
pub fn add(obj: &mut Object, file: &Path, funcs: &[Func]) -> Result<(), String> {
    // Objects are for the host.
    let endian = if cfg!(target_endian = "little") {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    // Version 4 keeps every string inline, so only addresses and section offsets need relocations.
    let enc = Encoding {
        format: Format::Dwarf32,
        version: 4,
        address_size: 8,
    };
    let dir = file.parent().unwrap_or(Path::new("."));
    let name = file.file_name().ok_or("source file has no name")?;
    let bytes = |s: &std::ffi::OsStr| s.to_string_lossy().into_owned().into_bytes();

    let mut dw = DwarfUnit::new(enc);
    let mut lp = LineProgram::new(
        enc,
        LineEncoding::default(),
        LineString::String(bytes(dir.as_os_str())),
        None,
        LineString::String(bytes(name)),
        None,
    );
    let dir_id = lp.default_directory();
    let fid = lp.add_file(LineString::String(bytes(name)), dir_id, None);

    let root = dw.unit.root();
    let cu = dw.unit.get_mut(root);
    cu.set(
        constants::DW_AT_producer,
        AttributeValue::String(b"yan".to_vec()),
    );
    cu.set(
        constants::DW_AT_language,
        AttributeValue::Language(constants::DW_LANG_C99),
    );
    cu.set(
        constants::DW_AT_name,
        AttributeValue::String(bytes(file.as_os_str())),
    );
    cu.set(
        constants::DW_AT_comp_dir,
        AttributeValue::String(bytes(dir.as_os_str())),
    );
    cu.set(
        constants::DW_AT_low_pc,
        AttributeValue::Address(Address::Constant(0)),
    );

    for (i, f) in funcs.iter().enumerate() {
        let start = Address::Symbol {
            symbol: i,
            addend: 0,
        };
        let die = dw.unit.add(root, constants::DW_TAG_subprogram);
        let die = dw.unit.get_mut(die);
        die.set(
            constants::DW_AT_name,
            AttributeValue::String(f.name.clone().into_bytes()),
        );
        die.set(constants::DW_AT_low_pc, AttributeValue::Address(start));
        die.set(constants::DW_AT_high_pc, AttributeValue::Udata(f.size));
        die.set(constants::DW_AT_external, AttributeValue::Flag(true));

        let mut prev = 0;
        let mut began = false;
        for &(off, line) in &f.rows {
            if line == 0 || line == prev {
                continue;
            }
            if !began {
                lp.begin_sequence(Some(start));
                began = true;
            }
            prev = line;
            let row = lp.row();
            row.address_offset = u64::from(off);
            row.file = fid;
            row.line = u64::from(line);
            row.is_statement = true;
            lp.generate_row();
        }
        if began {
            lp.end_sequence(f.size);
        }
    }
    dw.unit.line_program = lp;

    let blank = Section {
        data: EndianVec::new(endian),
        relocs: Vec::new(),
    };
    let mut sections = Sections::new(blank);
    dw.write(&mut sections).map_err(|e| e.to_string())?;

    let macho = obj.format() == BinaryFormat::MachO;
    let mut added = Vec::new();
    sections
        .for_each(|id, s| {
            if s.data.slice().is_empty() {
                return Ok::<_, String>(());
            }
            let n = id.name();
            let (segment, name) = if macho {
                (
                    obj.segment_name(StandardSegment::Debug).to_vec(),
                    format!("__{}", &n[1..]).into_bytes(),
                )
            } else {
                (Vec::new(), n.as_bytes().to_vec())
            };
            let sec = obj.add_section(segment, name, SectionKind::Debug);
            obj.set_section_data(sec, s.data.slice().to_vec(), 1);
            added.push((id, sec, s.relocs.clone()));
            Ok(())
        })
        .map_err(|e| e.to_string())?;

    for (_, sec, relocs) in &added {
        for r in relocs {
            let symbol = match r.target {
                RelocationTarget::Symbol(i) => funcs[i].symbol,
                RelocationTarget::Section(target) => {
                    let (_, t, _) = added
                        .iter()
                        .find(|(id, ..)| *id == target)
                        .ok_or("relocation to an empty section")?;
                    obj.section_symbol(*t)
                }
            };
            obj.add_relocation(
                *sec,
                ObjRelocation {
                    offset: r.offset as u64,
                    symbol,
                    addend: r.addend,
                    flags: RelocationFlags::Generic {
                        kind: RelocationKind::Absolute,
                        encoding: RelocationEncoding::Generic,
                        size: r.size * 8,
                    },
                },
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
