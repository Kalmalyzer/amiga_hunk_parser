extern crate byteorder;

use std::fmt;
use std::fs::File;
use byteorder::{BigEndian, ReadBytesExt};
use std::io;
use std::io::{Error, ErrorKind, Seek, Read};

const HUNK_HEADER: u32 = 1011;
// hunk types
const HUNK_UNIT: u32 = 999;
const HUNK_NAME: u32 = 1000;
const HUNK_CODE: u32 = 1001;
const HUNK_DATA: u32 = 1002;
const HUNK_BSS: u32 = 1003;
const HUNK_RELOC32: u32 = 1004;
const HUNK_DEBUG: u32 = 1009;
const HUNK_SYMBOL: u32 = 1008;
const HUNK_END: u32 = 1010;
const DEBUG_LINE: u32 = 0x4c494e45;

const HUNKF_CHIP: u32 = 1 << 30;
const HUNKF_FAST: u32 = 1 << 31;

pub struct HunkParser;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HunkType {
    Code,
    Data,
    Bss,
}

#[derive(Debug)]
pub struct RelocInfo32 {
    pub target: usize, 
    pub data: Vec<u32>,
}

pub struct Symbol {
    name: String,
    offset: u32,
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "0x{:08x} - {}\n", self.offset, self.name)
    }
}

pub struct SourceLine {
    line: u32,
    offset: u32,
}

impl fmt::Debug for SourceLine {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "0x{:08x} - {}\n", self.offset, self.line)
    }
}

#[derive(Debug)]
pub struct SourceFile {
    name: String,
    base_offset: u32,
    lines: Vec<SourceLine>,
}

#[derive(Debug)]
pub struct Hunk {
    pub mem_type: MemoryType,
    pub hunk_type: HunkType,
    pub alloc_size: usize,
    pub data_size: usize,
    pub code_data: Option<Vec<u8>>, 
    pub reloc_32: Option<Vec<RelocInfo32>>,
    pub symbols: Option<Vec<Symbol>>,
    pub line_debug_info: Option<Vec<SourceFile>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemoryType {
    Any,
    Chip,
    Fast,
}

struct SizesTypes {
    mem_type: MemoryType,
    size: usize,
}

impl HunkParser {
    fn skip_hunk(file: &mut File, name: &'static str) -> io::Result<()> {
        println!("Skipping {}\n", name);
        let seek_offset = try!(file.read_u32::<BigEndian>());
        file.seek(io::SeekFrom::Current(seek_offset as i64)).map(|_|())
    }

    fn get_size_type(t: u32) -> (usize, MemoryType) {
        let size = (t & 0x0fffffff) * 4;
        let mem_t = t & 0xf0000000;
        let mem_type = match mem_t {
            HUNKF_CHIP => MemoryType::Chip,
            HUNKF_FAST => MemoryType::Fast,
            _ => MemoryType::Any,
        };

        (size as usize, mem_type)
    }

    fn parse_bss(hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        let (size, _mem_type) = Self::get_size_type(try!(file.read_u32::<BigEndian>()));
        hunk.hunk_type = HunkType::Bss;
        hunk.data_size = size;
        Ok(())
    }

    fn parse_code_or_data(hunk_type: HunkType, hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        let (size, _mem_type) = Self::get_size_type(try!(file.read_u32::<BigEndian>()));
        let mut code_data: Vec<u8> = vec![0; size];
        dbg!(&hunk);

        hunk.data_size = size;
        hunk.hunk_type = hunk_type;

        try!(file.read(&mut code_data));

        hunk.code_data = Some(code_data);

        Ok(())
    }

    fn find_string_end(name: &[u8]) -> usize {
        for (i, val) in name.iter().enumerate() {
            if *val == 0 {
                return i;
            }
        }

        name.len()
    }

    fn read_name_size(file: &mut File, num_longs: u32) -> io::Result<String> {
        let len = num_longs * 4;
        let mut temp_buffer: [u8; 512] = [0; 512];
        let mut dest = &mut temp_buffer[..len as usize];
        try!(file.read_exact(dest));
        Ok(String::from_utf8_lossy(&dest[..Self::find_string_end(&dest)]).into_owned())
        //Ok(String::from_utf8_lossy(&dest).into_owned())
    }

    /*
    fn read_name(file: &mut File) -> Option<io::Result<String>> {
        let num_longs = try!(file.read_u32::<BigEndian>());
    }
    */

    fn parse_symbols(hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        let mut symbols = Vec::new();

        loop { 
            let num_longs = try!(file.read_u32::<BigEndian>());

            if num_longs == 0 {
                break;
            }

            let symbol = Symbol {
                name: try!(Self::read_name_size(file, num_longs)),
                offset: try!(file.read_u32::<BigEndian>()),
            };

            symbols.push(symbol);
        }

        if symbols.len() > 0 {
            symbols.sort_by(|a, b| a.offset.cmp(&b.offset));
            hunk.symbols = Some(symbols);
        }

        Ok(())
    }

    fn fill_debug_info(base_offset: u32, num_longs: u32, file: &mut File) -> io::Result<SourceFile> {
        let num_name_longs = try!(file.read_u32::<BigEndian>());
        let name = try!(Self::read_name_size(file, num_name_longs));
        let num_lines = (num_longs - num_name_longs - 1) / 2;
        let mut lines = Vec::with_capacity(num_lines as usize);

        for _ in 0..num_lines {
            let line_no = try!(file.read_u32::<BigEndian>()) & 0xffffff; // mask for SAS/C extra info
            let offset = try!(file.read_u32::<BigEndian>());
            lines.push(SourceLine {
                line: line_no,
                offset: base_offset + offset,
            });
        }

        Ok(SourceFile {
            name: name,
            base_offset: base_offset,
            lines: lines,
        })
    }

    fn parse_debug(hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        let num_longs = try!(file.read_u32::<BigEndian>()) - 2; // skip base offset and tag
        let base_offset = try!(file.read_u32::<BigEndian>());
        let debug_tag = try!(file.read_u32::<BigEndian>());

        // We only support debug line as debug format currently so skip if not found
        if debug_tag != DEBUG_LINE {
            try!(file.seek(io::SeekFrom::Current((num_longs * 4) as i64)).map(|_|()));
            return Ok(());
        }

        if let Some(ref mut debug_info) = hunk.line_debug_info {
            let source_file = try!(Self::fill_debug_info(base_offset, num_longs, file));
            debug_info.push(source_file);
        } else {
            let mut debug_info = Vec::new();
            let source_file = try!(Self::fill_debug_info(base_offset, num_longs, file));
            debug_info.push(source_file);
            hunk.line_debug_info = Some(debug_info);
        }

        return Ok(());
    }

    fn parse_reloc32(hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        let mut relocs = Vec::<RelocInfo32>::new();  

        loop {
            let count = try!(file.read_u32::<BigEndian>()) as usize;

            if count == 0 {
                break;
            }

            let target = try!(file.read_u32::<BigEndian>()) as usize;

            let mut reloc = RelocInfo32 {
                target: target,
                data: Vec::<u32>::with_capacity(count),
            };

            for _ in 0..count {
                reloc.data.push(try!(file.read_u32::<BigEndian>()));
            }

            relocs.push(reloc);
        }

        hunk.reloc_32 = Some(relocs);

        Ok(())
    }

    fn fill_hunk(hunk: &mut Hunk, file: &mut File) -> io::Result<()> {
        loop {
            let hunk_type = try!(file.read_u32::<BigEndian>());

            match hunk_type {
                HUNK_UNIT => { try!(Self::skip_hunk(file, "HUNK_UNIT")) }
                HUNK_NAME => { try!(Self::skip_hunk(file, "HUNK_NAME")) }
                HUNK_DEBUG => { try!(Self::parse_debug(hunk, file)) }
                HUNK_CODE => { try!(Self::parse_code_or_data(HunkType::Code, hunk, file)) }
                HUNK_DATA => { try!(Self::parse_code_or_data(HunkType::Data, hunk, file)) }
                HUNK_BSS => { try!(Self::parse_bss(hunk, file)) }
                HUNK_RELOC32 => { try!(Self::parse_reloc32(hunk, file)) }
                HUNK_SYMBOL => { try!(Self::parse_symbols(hunk, file)) }
                HUNK_END => {
                    return Ok(());
                }

                _ => {
                    println!("Unknown hunk type {:x}", hunk_type);
                    return Err(Error::new(ErrorKind::Other, "Unsupported hunk"));
                }
            }
        }
    }

    pub fn parse_file(filename: &str) -> Result<Vec<Hunk>, io::Error> {
        //let mut index = 0;

        let mut file = try!(File::open(filename));

        let hunk_header = try!(file.read_u32::<BigEndian>());
        if hunk_header != HUNK_HEADER  {
            return Err(Error::new(ErrorKind::Other, "Unable to find correct HUNK_HEADER"));
        };

        // Skip header/string section
        try!(file.read_u32::<BigEndian>());

        let table_size = try!(file.read_u32::<BigEndian>()) as i32;
        let first_hunk = try!(file.read_u32::<BigEndian>()) as i32;
        let last_hunk = try!(file.read_u32::<BigEndian>()) as i32;

        if table_size < 0 || first_hunk < 0 || last_hunk < 0 {
            return Err(Error::new(ErrorKind::Other, "Invalid sizes for hunks"));
        }

        let hunk_count = (last_hunk - first_hunk + 1) as usize;

        let mut hunk_table = Vec::with_capacity(hunk_count);

        for _ in 0..hunk_count {
            let (size, mem_type) = Self::get_size_type(try!(file.read_u32::<BigEndian>()));
            hunk_table.push(SizesTypes {
                mem_type: mem_type,
                size: size 
            });
        }

        let mut hunks = Vec::with_capacity(hunk_count);

        for i in 0..hunk_count {
            let mut hunk = Hunk {
                mem_type: hunk_table[i].mem_type, 
                    hunk_type: HunkType::Bss,
                    alloc_size: hunk_table[i].size as usize,
                    data_size: 0,
                    code_data: None, 
                    reloc_32: None, 
                    symbols: None, 
                    line_debug_info: None,
            };

            try!(Self::fill_hunk(&mut hunk, &mut file));


            hunks.push(hunk);
        }

        // dump info

        //for hunk in hunks {
        //   println!("type {:?} - size {} - alloc_size {}", hunk.hunk_type, hunk.data_size, hunk.alloc_size); 
        //}

        //println!("b {}", hunk_header);
        Ok(hunks)
    }
}

#[test]
fn test_memory_types() {
    let hunks = HunkParser::parse_file("testdata/test.memory_types.amiga.exe").unwrap();

    assert_eq!(3, hunks.len());
    assert_eq!(MemoryType::Fast, hunks[0].mem_type);
    assert_eq!(MemoryType::Chip, hunks[1].mem_type);
    assert_eq!(MemoryType::Any, hunks[2].mem_type);
}

#[test]
fn test_symbols() {
    let hunks = HunkParser::parse_file("testdata/test.symbols.amiga.exe").unwrap();

    assert_eq!(2, hunks.len());
    assert_eq!(4usize, hunks[0].symbols.as_ref().unwrap().len());
    assert_eq!("code_start", hunks[0].symbols.as_ref().unwrap()[0].name);
    assert_eq!(0u32, hunks[0].symbols.as_ref().unwrap()[0].offset);
    assert_eq!("code_middle2", hunks[0].symbols.as_ref().unwrap()[1].name);
    assert_eq!(2u32, hunks[0].symbols.as_ref().unwrap()[1].offset);
    assert_eq!("code_middle1", hunks[0].symbols.as_ref().unwrap()[2].name);
    assert_eq!(2u32, hunks[0].symbols.as_ref().unwrap()[2].offset);
    assert_eq!("code_end", hunks[0].symbols.as_ref().unwrap()[3].name);
    assert_eq!(4u32, hunks[0].symbols.as_ref().unwrap()[3].offset);

    assert_eq!(2usize, hunks[1].symbols.as_ref().unwrap().len());
    assert_eq!("data_start", hunks[1].symbols.as_ref().unwrap()[0].name);
    assert_eq!(0u32, hunks[1].symbols.as_ref().unwrap()[0].offset);
    assert_eq!("data_end", hunks[1].symbols.as_ref().unwrap()[1].name);
    assert_eq!(2u32, hunks[1].symbols.as_ref().unwrap()[1].offset);
}

#[test]
fn test_parse_file() {
    let hunks = HunkParser::parse_file("testdata/test.amiga.exe").unwrap();

    assert_eq!(4, hunks.len());

    assert_eq!(HunkType::Code, hunks[0].hunk_type);
    assert_eq!(MemoryType::Fast, hunks[0].mem_type);
    assert_eq!(8, hunks[0].alloc_size);
    assert_eq!(8, hunks[0].data_size);
    assert!(hunks[0].reloc_32.is_none());
    assert_eq!(3usize, hunks[0].symbols.as_ref().unwrap().len());
 
    assert_eq!(HunkType::Data, hunks[1].hunk_type);
    assert_eq!(MemoryType::Any, hunks[1].mem_type);
    assert_eq!(8, hunks[1].alloc_size);
    assert_eq!(8, hunks[1].data_size);
    assert!(hunks[1].reloc_32.is_none());
    assert_eq!(1usize, hunks[1].symbols.as_ref().unwrap().len());

    assert_eq!(HunkType::Data, hunks[2].hunk_type);
    assert_eq!(MemoryType::Chip, hunks[2].mem_type);
    assert_eq!(24, hunks[2].alloc_size);
    assert_eq!(24, hunks[2].data_size);
    assert!(hunks[2].reloc_32.is_none());
    assert!(hunks[2].symbols.is_none());

    assert_eq!(HunkType::Bss, hunks[3].hunk_type);
    assert_eq!(MemoryType::Any, hunks[3].mem_type);
    assert_eq!(12, hunks[3].alloc_size);
    assert_eq!(12, hunks[3].data_size);
    assert!(hunks[3].reloc_32.is_none());
    assert_eq!(2usize, hunks[3].symbols.as_ref().unwrap().len());
}
