use calamine::DataType;
use calamine::Reader;
use calamine::{open_workbook_auto, Sheets};

use std::fmt;
use std::path::PathBuf;
use structopt::StructOpt;

use regex::RegexBuilder;

/// Select sheet by id or by name.
#[derive(Clone, Debug)]
pub enum SheetSelector {
    ById(usize),
    ByName(String),
}

impl SheetSelector {
    pub fn find_in<'a>(&self, sheetnames: &'a [String]) -> Result<&'a String, String> {
        match self {
            SheetSelector::ById(id) => {
                if *id >= sheetnames.len() {
                    Err(format!(
                        "sheet id `{}` is not valid - only **{}** sheets avaliable!",
                        id,
                        sheetnames.len()
                    ))
                } else {
                    Ok(&sheetnames[*id])
                }
            }
            SheetSelector::ByName(name) => {
                if let Some(name) = sheetnames.iter().find(|s| *s == name) {
                    Ok(name)
                } else {
                    let msg = format!(
                        "sheet name `{}` is not in ({})",
                        name,
                        sheetnames.join(", ")
                    );
                    Err(msg)
                }
            }
        }
    }
}

impl std::str::FromStr for SheetSelector {
    type Err = String;
    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str.parse() {
            Ok(id) => Ok(SheetSelector::ById(id)),
            Err(_) => Ok(SheetSelector::ByName(str.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Delimiter(pub u8);

/// Delimiter represents values that can be passed from the command line that
/// can be used as a field delimiter in CSV data.
///
/// Its purpose is to ensure that the Unicode character given decodes to a
/// valid ASCII character as required by the CSV parser.
impl Delimiter {
    pub fn as_byte(&self) -> u8 {
        self.0
    }
    pub fn as_char(&self) -> char {
        self.0 as char
    }
    pub fn to_file_extension(&self) -> String {
        match self.0 {
            b'\t' => "tsv".into(),
            _ => "csv".to_string(),
        }
    }
}

impl fmt::Display for Delimiter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

impl std::str::FromStr for Delimiter {
    type Err = String;
    fn from_str(str: &str) -> Result<Delimiter, Self::Err> {
        match str {
            r"\t" => Ok(Delimiter(b'\t')),
            r"\n" => Ok(Delimiter(b'\n')),
            s => {
                if s.len() != 1 {
                    let msg = format!("Could not convert '{}' to a single ASCII character.", s);
                    return Err(msg);
                }
                let c = s.chars().next().unwrap();
                if c.is_ascii() {
                    Ok(Delimiter(c as u8))
                } else {
                    let msg = format!("Could not convert '{}' to ASCII delimiter.", c);
                    Err(msg)
                }
            }
        }
    }
}

/// A fast Excel-like spreadsheet to CSV coverter in Rust.
///
/// A simple usage like this:
///
/// ```
/// xlsx2csv input.xlsx sheet1.csv sheet2.csv
/// ```
///
/// If no output position args setted, it'll write first sheet to stdout.
/// So the two commands are equal:
///
/// - `xlsx2csv input.xlsx sheet1.csv`
///
/// - `xlsx2csv input.xlsx > sheet1.csv`.
///
/// If you want to select specific sheet to stdout, use `-s/--select <id or name>` (id is 0-based):
///
/// `xlsx2csv input.xlsx -s 1`
///
/// In previous command, it'll output the second sheet to stdout.
///
/// If there's many sheets that you don't wanna set filename for each,
/// use `-u` to write with sheetnames.
///
/// ```
/// xlsx2csv input.xlsx -u
/// ```
///
/// If you want to write to directory other than `.`, use `-w/--workdir` along with `-u` option.
///
/// ```
/// xlsx2csv input.xlsx -u -w test/
/// ```
///
/// The filename extension is detemined by delimiter, `,` to `.csv`, `\t` to `.tsv`, others will treat as ','.
///
/// By default, it will output all sheets, but if you want to select by sheet names with regex match, use `-I/--include` to include only matching, and `-X/--exclude` to exclude matching.
/// You could also combine these two option with *include-first-exclude-after* order.
///
/// ```
/// xlsx2csv input.xlsx -I '\S{3,}' -X 'Sheet'
/// ```
#[derive(Debug, StructOpt)]
struct Opt {
    /// Input Excel-like files, supports: .xls .xlsx .xlsb .xlsm .ods
    xlsx: PathBuf,
    /// Output each sheet to sperated file.
    ///
    /// If not setted, output first sheet to stdout.
    output: Vec<PathBuf>,
    /// List sheet names by id.
    #[structopt(short, long, conflicts_with_all = &["output", "select", "use_sheet_names"])]
    list: bool,
    /// Select sheet by name or id in output, only used when output to stdout.
    #[structopt(short, long, conflicts_with = "output")]
    select: Option<SheetSelector>,
    /// Use sheet names as output filename prefix (in current dir or --workdir).
    #[structopt(short, long, alias = "sheet", conflicts_with = "output")]
    use_sheet_names: bool,
    /// Output files location if `--use-sheet-names` setted
    #[structopt(short, long, conflicts_with = "output", requires = "use-sheet-names")]
    workdir: Option<PathBuf>,
    /// A regex pattern for matching sheetnames to include, used with '-u'.
    #[structopt(short = "I", long, requires = "use-sheet-names")]
    include: Option<String>,
    /// A regex pattern for matching sheetnames to exclude, used with '-u'.
    #[structopt(short = "X", long, requires = "use-sheet-names")]
    exclude: Option<String>,
    /// Rgex case insensitivedly.
    ///
    /// When this flag is provided, the include and exclude patterns will be searched case insensitively. used with '-u'.
    #[structopt(short = "i", long, requires = "use-sheet-names")]
    ignore_case: bool,
    /// Delimiter for output.
    ///
    /// If `use-sheet-names` setted, it will control the output filename extension: , -> csv, \t -> tsv
    #[structopt(short, long, default_value = ",")]
    delimiter: Delimiter,
}

fn worksheet_to_csv<W: std::io::Write>(
    workbook: &mut Sheets,
    sheet: &str,
    wtr: &mut csv::Writer<W>,
) {
    let range = workbook
        .worksheet_range(&sheet)
        .expect(&format!("find sheet {}", sheet))
        .expect("get range");
    let size = range.get_size();
    if size.0 == 0 || size.1 == 0 {
        //panic!("Worksheet range sizes should not be 0, continue");
        return;
    }
    let rows = range.rows();
    for row in rows {
        let cols: Vec<String> = row
            .iter()
            .map(|c| match *c {
                DataType::Int(ref c) => format!("{}", c),
                DataType::Float(ref c) => format!("{}", c),
                DataType::String(ref c) => format!("{}", c),
                DataType::Bool(ref c) => format!("{}", c),
                _ => "".to_string(),
            })
            .collect();
        wtr.write_record(&cols).unwrap();
    }
    wtr.flush().unwrap();
}
fn main() {
    let opt = Opt::from_args();
    let mut workbook: Sheets = open_workbook_auto(&opt.xlsx).expect("open file");
    let sheetnames = workbook.sheet_names().to_vec();
    if sheetnames.is_empty() {
        panic!("input file has zero sheet!");
    }

    if opt.list {
        for sheet in sheetnames {
            println!("{}", sheet);
        }
        return;
    }

    if opt.use_sheet_names {
        let ignore_case = opt.ignore_case;
        let include_pattern = opt.include.map(|p| {
            RegexBuilder::new(&p)
                .case_insensitive(ignore_case)
                .build()
                .unwrap()
        });
        let exclude_pattern = opt.exclude.map(|p| {
            RegexBuilder::new(&p)
                .case_insensitive(ignore_case)
                .build()
                .unwrap()
        });
        let ext = opt.delimiter.to_file_extension();
        let workdir = opt.workdir.unwrap_or(PathBuf::new());
        for sheet in sheetnames
            .iter()
            .filter(|name| {
                include_pattern
                    .as_ref()
                    .map(|r| r.is_match(name))
                    .unwrap_or(true)
            })
            .filter(|name| {
                exclude_pattern
                    .as_ref()
                    .map(|r| !r.is_match(name))
                    .unwrap_or(true)
            })
        {
            let output = workdir.join(&format!("{}.{}", sheet, ext));
            println!("{}", output.display());
            let mut wtr = csv::WriterBuilder::new()
                .delimiter(opt.delimiter.as_byte())
                .from_path(output)
                .expect("open file for output");
            worksheet_to_csv(&mut workbook, &sheet, &mut wtr);
        }
    } else if opt.output.is_empty() {
        let stdout = std::io::stdout();
        let mut wtr = csv::WriterBuilder::new()
            .delimiter(opt.delimiter.as_byte())
            .from_writer(stdout);

        if let Some(select) = opt.select {
            let name = select.find_in(&sheetnames).expect("invalid selector");
            worksheet_to_csv(&mut workbook, &name, &mut wtr);
        } else {
            worksheet_to_csv(&mut workbook, &sheetnames[0], &mut wtr);
        }
    } else {
        for (sheet, output) in sheetnames.iter().zip(opt.output.iter()) {
            println!("{}", output.display());
            let mut wtr = csv::WriterBuilder::new()
                .delimiter(opt.delimiter.as_byte())
                .from_path(output)
                .expect("open file for output");
            worksheet_to_csv(&mut workbook, &sheet, &mut wtr);
        }
    }
}
