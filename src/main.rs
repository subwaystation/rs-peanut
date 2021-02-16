use std::fs::File;
use std::io::prelude::*;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use bstr::ByteSlice;

use gfa::{
    cigar::CIGAR,
    gafpaf::parse_gaf,
    optfields::{OptFieldVal, OptFields, OptionalFields},
};

use clap::{App, Arg};

type GAF = gfa::gafpaf::GAF<OptionalFields>;

fn main() {
    let arguments = App::new("peanut")
        .version("0.1.0")
        .author("Simon Heumos <simon.heumos@qbic.uni-tuebingen.de")
        .about("Evaluate GAF alignment quality")
        .arg(
            Arg::with_name("GAF")
                .short("g")
                .long("gaf")
                .required(true)
                .takes_value(true)
                .help("Input GAF file of which to evaluate the alignment quality.")        
        )
        .arg(
            Arg::with_name("BED")
                .short("b")
                .long("bed")
                .required(false)
                .takes_value(true)
                .help("Output BED file to which the regions of the non query sequence matches should be written to.")        
        )
        .get_matches();

    let gaf_filename = arguments.value_of("GAF").unwrap();
    let gaf_file_exists = std::path::Path::new(gaf_filename).exists();
    if !gaf_file_exists {
        eprintln!(
            "[peanut::main::error]: GAF file {} does not exist!",
            gaf_filename
        );
        std::process::exit(1);
    }

    let bed_filename;
    let bed_nonaln = arguments.value_of("BED");
    let mut do_bed_nonaln: bool = false;
    let path: &std::path::Path;
    let display: std::path::Display;
    let mut bed_file_option: std::option::Option<std::fs::File> = None;

    if bed_nonaln.is_some() {
        bed_filename = bed_nonaln.unwrap();
        do_bed_nonaln = true;
        path = Path::new(bed_filename);
        display = path.display();

        // Open a file in write-only mode, returns `io::Result<File>`
        bed_file_option = match File::create(&path) {
            Err(why) => panic!(
                "[peanut::main::error]: Couldn't create {}: {}!",
                display, why
            ),
            Ok(bed_file) => Some(bed_file),
        };
    }

    let gaf_file = File::open(gaf_filename).unwrap();
    let mut lines = BufReader::new(gaf_file);
    let mut line: Vec<u8> = Vec::new();

    let mut cur_seq_len: usize = 0;
    let mut cur_seq_name: Vec<u8> = vec![0; 0];
    let mut first_line_seen: bool = false;
    let mut seq_name: Vec<u8>;
    let mut _seq_len: usize = 0;
    let mut _aln_start: usize = 0;

    let mut total_seq_len: usize = 0;
    let mut total_aln_len: usize = 0;
    let mut total_multi_aln_len: usize = 0;
    let mut total_uniq_aln_len: usize = 0;
    let mut total_non_aln_len: usize = 0;
    let mut nuc_bv: Vec<bool> = vec![false; 0];
    let mut nuc_bv_multi: Vec<bool> = vec![false; 0];

    loop {
        line.clear();
        let bytes_read = lines.read_until(0xA, &mut line);
        if bytes_read.is_err() || bytes_read.unwrap() == 0 {
            break;
        }
        let fields: bstr::Split = line[0..line.len()].split_str(b"\t");
        if let Some::<GAF>(gaf) = parse_gaf(fields) {
            let opt_fields = gaf.optional;
            let cigar = get_cigar(&opt_fields).unwrap();

            seq_name = gaf.seq_name;
            _seq_len = gaf.seq_len;
            _aln_start = gaf.seq_range.0;

            if !first_line_seen {
                first_line_seen = true;
                cur_seq_name = seq_name;
                cur_seq_len = _seq_len;
                nuc_bv = vec![false; cur_seq_len];
                nuc_bv_multi = vec![false; cur_seq_len];
                eval_cigar(&cigar, &_aln_start, &mut nuc_bv, &mut nuc_bv_multi);
            } else if seq_name != cur_seq_name {
                // finish the current one
                total_seq_len += cur_seq_len;
                let aln_len: usize = nuc_bv.iter().filter(|&b| *b).count();
                total_aln_len += aln_len;
                let multi_aln_len: usize = nuc_bv_multi.iter().filter(|&b| *b).count();
                total_multi_aln_len += multi_aln_len;
                total_uniq_aln_len += aln_len - multi_aln_len;
                total_non_aln_len += cur_seq_len - aln_len;
                // do we need to write to BED?
                // TODO
                if do_bed_nonaln {
                    write_nonaln_to_bed(&mut bed_file_option, &nuc_bv, &cur_seq_name);
                }

                nuc_bv = vec![false; _seq_len];
                nuc_bv_multi = vec![false; _seq_len];

                cur_seq_len = _seq_len;
                cur_seq_name = seq_name;
                eval_cigar(&cigar, &_aln_start, &mut nuc_bv, &mut nuc_bv_multi);
            } else {
                eval_cigar(&cigar, &_aln_start, &mut nuc_bv, &mut nuc_bv_multi);
            }
        } else {
            eprintln!(
                "[peanut::main::error]: Error parsing GAF line {}!",
                line.as_bstr()
            );
        }
    }

    // we have to add the last step
    total_seq_len += cur_seq_len;
    let aln_len: usize = nuc_bv.iter().filter(|&b| *b).count();
    total_aln_len += aln_len;
    let multi_aln_len: usize = nuc_bv_multi.iter().filter(|&b| *b).count();
    total_multi_aln_len += multi_aln_len;
    total_uniq_aln_len += aln_len - multi_aln_len;
    total_non_aln_len += cur_seq_len - aln_len;
    // do we need to write to BED?
    // TODO
    if do_bed_nonaln {
        write_nonaln_to_bed(&mut bed_file_option, &nuc_bv, &cur_seq_name);
    }

    let ratio_qsc: f64 = total_aln_len as f64 / total_seq_len as f64;
    print!("{}", ratio_qsc);

    let ratio_uniq: f64 = total_uniq_aln_len as f64 / total_seq_len as f64;
    print!("\t{}", ratio_uniq);

    let ratio_multi: f64 = total_multi_aln_len as f64 / total_seq_len as f64;
    print!("\t{}", ratio_multi);

    let ratio_non: f64 = total_non_aln_len as f64 / total_seq_len as f64;
    println!("\t{}", ratio_non);
}

fn get_cigar<T: OptFields>(opts: &T) -> Option<CIGAR> {
    let cg = opts.get_field(b"cg")?;
    if let OptFieldVal::Z(cg) = &cg.value {
        CIGAR::from_bytestring(&cg)
    } else {
        None
    }
}

fn eval_cigar(
    cigar: &gfa::cigar::CIGAR,
    aln_start: &usize,
    nuc_bv: &mut Vec<bool>,
    nuc_bv_multi: &mut Vec<bool>,
) {
    let cigar_iter = cigar.iter();
    let mut idx: usize = 0;
    for (len, op) in cigar_iter {
        // we have seen a match!
        use gfa::cigar::CIGAROp as Op;
        if matches!(op, Op::E) {
            // did we already mark this position?
            for offset in 0..len {
                let pos: usize = idx + aln_start + offset as usize;
                let nuc_b: bool = nuc_bv[pos];
                if nuc_b {
                    nuc_bv_multi[pos] = true;
                } else {
                    nuc_bv[pos] = true;
                }
            }
        }
        if matches!(op, Op::E | Op::M | Op::X | Op::I) {
            idx += len as usize;
        }
    }
}

fn write_nonaln_to_bed(
    bed_file_option: &mut std::option::Option<std::fs::File>,
    nuc_bv: &Vec<bool>,
    cur_seq_name: &Vec<u8>,
) {
    let mut bed_file: &std::fs::File = bed_file_option.as_mut().unwrap();

    let mut chrom_start = 0;
    let mut chrom_end = 0;
    let mut last_nuc_was_false = false;
    for nuc_idx in 0..nuc_bv.len() {
        let nuc_b: bool = nuc_bv[nuc_idx];
        if nuc_b {
            // we need to finish this one
            if last_nuc_was_false {
                match bed_file.write_all(cur_seq_name.as_bytes()) {
                    Err(why) => panic!("[peanut::main::error]: Couldn't write to BED: {}!", why),
                    Ok(_) => (),
                }
                match bed_file.write_all(b"\t") {
                    Err(why) => panic!("[peanut::main::error]: Couldn't write to BED: {}!", why),
                    Ok(_) => (),
                }
                
            }
        } else {

        }
    }
    match bed_file.write_all(b"\n") {
        Err(why) => panic!("[peanut::main::error]: Couldn't write to BED: {}!", why),
        Ok(_) => (),
    }
}

// cargo fmt --all -- src/*.rs && cargo check && cargo clippy -- -D warnings && cargo build && cargo build --release
