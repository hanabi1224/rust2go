use std::io::Cursor;

use clap::Parser;
use rust2go_common::raw_file::RawRsFile;

#[derive(Parser, Debug, Default, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path of source rust file
    #[arg(short, long)]
    pub src: String,

    /// Path of destination go file
    #[arg(short, long)]
    pub dst: String,

    /// With or without go main function
    #[arg(long, default_value = "false")]
    pub without_main: bool,

    /// Go 1.18 compatible
    #[arg(long, default_value = "false")]
    pub go118: bool,

    /// Disable auto format go file
    #[arg(long, default_value = "false")]
    pub no_fmt: bool,
}

pub fn generate(args: &Args) {
    // Read and parse rs file.
    let file_content = std::fs::read_to_string(&args.src).expect("Unable to read file");
    let raw_file = RawRsFile::new(file_content);

    // Convert to Ref structs and write to output file.
    let (name_mapping, ref_content) = raw_file
        .convert_structs_to_ref()
        .expect("Unable to convert to ref");
    std::fs::write(&args.dst, ref_content.to_string()).expect("Unable to write file");

    // Convert output file with cbindgen.
    let mut cbuilder = cbindgen::Builder::new()
        .with_language(cbindgen::Language::C)
        .with_src(&args.dst)
        .with_header("// Generated by rust2go. Please DO NOT edit this C part manually.");
    for name in name_mapping.values() {
        cbuilder = cbuilder.include_item(name.to_string());
    }
    let mut output = Vec::<u8>::new();
    cbuilder
        .generate()
        .expect("Unable to generate bindings")
        .write(Cursor::new(&mut output));

    // Convert headers into golang.
    let mut output = String::from_utf8(output).expect("Unable to convert to string");

    let traits = raw_file.convert_trait().unwrap();
    let use_shm = traits
        .iter()
        .any(|t| t.fns().iter().any(|f| f.mem_call_id().is_some()));
    let use_cgo = traits
        .iter()
        .any(|t| t.fns().iter().any(|f| f.mem_call_id().is_none()));
    traits
        .iter()
        .for_each(|t| output.push_str(&t.generate_c_callbacks()));
    if use_shm {
        output.push_str(RawRsFile::go_shm_include());
    }

    let import_shm = if use_shm {
        "mem_ring \"github.com/ihciah/rust2go/mem-ring\"\n\"github.com/panjf2000/ants/v2\"\n"
    } else {
        ""
    };
    let import_cgo = if use_cgo { "\"runtime\"\n" } else { "" };

    let import_118 = if args.go118 { "\"reflect\"\n" } else { "" };
    let mut go_content = format!(
    "package main\n\n/*\n{output}*/\nimport \"C\"\nimport (\n\"unsafe\"\n{import_cgo}{import_118}{import_shm})\n"
);
    let levels = raw_file.convert_structs_levels().unwrap();
    traits.iter().for_each(|t| {
        go_content.push_str(&t.generate_go_interface());
        go_content.push_str(&t.generate_go_exports(&levels));
    });
    go_content.push_str(
        &raw_file
            .convert_structs_to_go(&levels, args.go118)
            .expect("Unable to generate go structs"),
    );
    if use_shm {
        go_content.push_str(RawRsFile::go_shm_ring_init());
    }
    if !args.without_main {
        go_content.push_str("func main() {}\n");
    }

    std::fs::write(&args.dst, go_content).expect("Unable to write file");

    if !args.no_fmt {
        std::process::Command::new("go")
            .arg("fmt")
            .arg(&args.dst)
            .status()
            .unwrap();
    }
}
