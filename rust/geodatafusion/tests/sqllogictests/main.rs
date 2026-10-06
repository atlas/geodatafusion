//! sqllogictest runner comparing geodatafusion against PostGIS.
//!
//! See `.claude/skills/postgis-parity-tests/SKILL.md` and `tests/sqllogictests/README.md`.
//!
//! ```text
//! cargo slt [OPTIONS] [FILTER...]
//! # alias for: cargo test -p geodatafusion --all-features --test sqllogictests -- ...
//!
//! FILTER               Function / file name, e.g. `st_area` or `postgis_docs/st_area`.
//!                      Exact file-stem matches win; otherwise substring match on the path.
//! --postgis            Run the files against PostGIS instead (checks the expectations).
//! --complete           Re-record expected output from PostGIS (rewrites the .slt files).
//! --update-parity      Write the current pass counts to parity.txt.
//! --verbose, -v        Print every failure, even when many files are selected.
//! --list               List the selected files and exit.
//! ```

mod datafusion_engine;
mod postgis;
mod render;

use std::collections::BTreeMap;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use futures::{FutureExt, StreamExt};
use sqllogictest::{
    AsyncDB, DefaultColumnType, MakeConnection, Record, Runner, default_normalizer,
    default_validator, parse_file, strict_column_validator,
};

/// Error type shared by both engines.
#[derive(Debug)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

macro_rules! impl_from_error {
    ($($t:ty),*) => {
        $(impl From<$t> for EngineError {
            fn from(e: $t) -> Self {
                EngineError(e.to_string())
            }
        })*
    };
}

impl_from_error!(
    datafusion::error::DataFusionError,
    arrow_schema::ArrowError,
    geoarrow_schema::error::GeoArrowError,
    tokio_postgres::Error
);

/// Files with more than this many selected trigger summary-only output unless `--verbose`.
const DETAIL_THRESHOLD: usize = 5;

#[derive(Default)]
struct Args {
    filters: Vec<String>,
    postgis: bool,
    complete: bool,
    update_parity: bool,
    verbose: bool,
    list: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--postgis" => args.postgis = true,
            "--complete" => args.complete = true,
            "--update-parity" => args.update_parity = true,
            "--verbose" | "-v" => args.verbose = true,
            "--list" => args.list = true,
            // Ignore flags cargo/libtest may pass through (e.g. --nocapture, --quiet).
            a if a.starts_with('-') => {}
            a => args.filters.push(a.trim_end_matches(".slt").to_lowercase()),
        }
    }
    args
}

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/sqllogictests")
}

fn slt_dir() -> PathBuf {
    test_dir().join("slt")
}

fn parity_path() -> PathBuf {
    test_dir().join("parity.txt")
}

/// A test file, identified by its path relative to `slt/` without extension.
#[derive(Clone)]
struct TestFile {
    name: String,
    path: PathBuf,
}

fn discover(dir: &Path, out: &mut Vec<TestFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover(&path, out);
        } else if path.extension().is_some_and(|e| e == "slt") {
            let rel = path
                .strip_prefix(slt_dir())
                .expect("discovered under slt_dir")
                .with_extension("");
            out.push(TestFile {
                name: rel.to_string_lossy().replace('\\', "/"),
                path,
            });
        }
    }
}

fn select(all: &[TestFile], filters: &[String]) -> Vec<TestFile> {
    if filters.is_empty() {
        return all.to_vec();
    }
    let mut selected: Vec<TestFile> = vec![];
    for filter in filters {
        let stem = |f: &TestFile| f.name.rsplit('/').next().unwrap_or(&f.name).to_lowercase();
        let exact: Vec<&TestFile> = all
            .iter()
            .filter(|f| stem(f) == *filter || f.name.to_lowercase() == *filter)
            .collect();
        let matches: Vec<&TestFile> = if exact.is_empty() {
            all.iter()
                .filter(|f| f.name.to_lowercase().contains(filter.as_str()))
                .collect()
        } else {
            exact
        };
        for m in matches {
            if !selected.iter().any(|s| s.name == m.name) {
                selected.push(m.clone());
            }
        }
    }
    selected.sort_by(|a, b| a.name.cmp(&b.name));
    selected
}

struct FileResult {
    name: String,
    passed: usize,
    total: usize,
    failures: Vec<String>,
}

/// Wraps an engine so that a panicking UDF fails the record instead of the whole run.
struct CatchUnwind<D>(D);

#[async_trait::async_trait]
impl<D: AsyncDB<Error = EngineError, ColumnType = DefaultColumnType> + Send> AsyncDB
    for CatchUnwind<D>
{
    type Error = EngineError;
    type ColumnType = DefaultColumnType;

    async fn run(
        &mut self,
        sql: &str,
    ) -> Result<sqllogictest::DBOutput<Self::ColumnType>, Self::Error> {
        match AssertUnwindSafe(self.0.run(sql)).catch_unwind().await {
            Ok(result) => result,
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "<non-string panic>".to_string());
                Err(EngineError(format!("panicked: {msg}")))
            }
        }
    }

    async fn shutdown(&mut self) {
        self.0.shutdown().await
    }

    fn engine_name(&self) -> &str {
        self.0.engine_name()
    }

    async fn sleep(dur: std::time::Duration) {
        tokio::time::sleep(dur).await
    }
}

async fn run_file<M>(file: TestFile, make_conn: M) -> FileResult
where
    M: MakeConnection,
    M::Conn: AsyncDB<ColumnType = DefaultColumnType>,
{
    let mut result = FileResult {
        name: file.name.clone(),
        passed: 0,
        total: 0,
        failures: vec![],
    };
    let records = match parse_file::<DefaultColumnType>(&file.path) {
        Ok(records) => records,
        Err(e) => {
            result.total = 1;
            result.failures.push(format!("parse error: {e}"));
            return result;
        }
    };
    let mut runner = Runner::new(make_conn);
    for record in records {
        let counted = matches!(record, Record::Query { .. } | Record::Statement { .. });
        let outcome = runner.run_async(record).await;
        if counted {
            result.total += 1;
            match outcome {
                Ok(_) => result.passed += 1,
                Err(e) => result.failures.push(e.display(false).to_string()),
            }
        }
    }
    runner.shutdown_async().await;
    result
}

fn read_parity() -> BTreeMap<String, (usize, usize)> {
    let Ok(contents) = std::fs::read_to_string(parity_path()) else {
        return BTreeMap::new();
    };
    contents
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (name, counts) = l.split_once(' ')?;
            let (passed, total) = counts.trim().split_once('/')?;
            Some((
                name.to_string(),
                (passed.parse().ok()?, total.parse().ok()?),
            ))
        })
        .collect()
}

fn write_parity(parity: &BTreeMap<String, (usize, usize)>) {
    let mut out = String::from(
        "# Passing records per sqllogictest file (passed/total) for geodatafusion.\n\
         # Generated by `cargo slt --update-parity`.\n\
         # The test run fails if these numbers don't match, so regressions are caught and\n\
         # improvements are recorded.\n",
    );
    for (name, (passed, total)) in parity {
        out.push_str(&format!("{name} {passed}/{total}\n"));
    }
    std::fs::write(parity_path(), out).expect("failed to write parity.txt");
}

fn concurrency() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

async fn complete(files: Vec<TestFile>) -> ExitCode {
    let mut failed = false;
    let results = futures::stream::iter(files)
        .map(|file| async move {
            let mut runner = Runner::new(postgis::PostGIS::connect);
            let result = runner
                .update_test_file(
                    &file.path,
                    " ",
                    default_validator,
                    default_normalizer,
                    strict_column_validator,
                )
                .await
                .map_err(|e| e.to_string())
                .and_then(|()| drop_error_messages(&file.path));
            runner.shutdown_async().await;
            (file.name, result)
        })
        .buffer_unordered(concurrency())
        .collect::<Vec<_>>()
        .await;
    for (name, result) in results {
        match result {
            Ok(()) => println!("recorded {name}"),
            Err(e) => {
                failed = true;
                println!("FAILED to record {name}: {e}");
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Rewrites `query error <message>` and `statement error <message>` records as plain `query
/// error` / `statement error`.
///
/// The recorded message is PostGIS's, which geodatafusion's never matches, and expectations only
/// assert that the query fails.
fn drop_error_messages(path: &Path) -> Result<(), String> {
    let contents = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let rewritten: Vec<&str> = contents
        .lines()
        .map(|line| {
            if line.starts_with("query error ") {
                "query error"
            } else if line.starts_with("statement error ") {
                "statement error"
            } else {
                line
            }
        })
        .collect();
    std::fs::write(path, rewritten.join("\n") + "\n").map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = parse_args();

    let mut all = vec![];
    discover(&slt_dir(), &mut all);
    all.sort_by(|a, b| a.name.cmp(&b.name));
    let files = select(&all, &args.filters);

    if files.is_empty() {
        eprintln!(
            "No .slt files match {:?} under {}",
            args.filters,
            slt_dir().display()
        );
        return ExitCode::FAILURE;
    }
    if args.list {
        for f in &files {
            println!("{}", f.name);
        }
        return ExitCode::SUCCESS;
    }
    if args.complete {
        return complete(files).await;
    }

    let detailed = args.verbose || files.len() <= DETAIL_THRESHOLD;
    let engine = if args.postgis {
        "PostGIS"
    } else {
        "geodatafusion"
    };

    let mut results: Vec<FileResult> = futures::stream::iter(files)
        .map(|file| {
            let postgis = args.postgis;
            tokio::spawn(async move {
                if postgis {
                    run_file(file, || async {
                        postgis::PostGIS::connect().await.map(CatchUnwind)
                    })
                    .await
                } else {
                    run_file(file, || async {
                        Ok::<_, EngineError>(CatchUnwind(datafusion_engine::GeoDataFusion::new()))
                    })
                    .await
                }
            })
        })
        .buffer_unordered(concurrency())
        .map(|r| r.expect("test task panicked"))
        .collect()
        .await;
    results.sort_by(|a, b| a.name.cmp(&b.name));

    let (mut passed, mut total, mut files_passed) = (0, 0, 0);
    for r in &results {
        passed += r.passed;
        total += r.total;
        if r.passed == r.total {
            files_passed += 1;
        }
        if detailed {
            let status = if r.passed == r.total { "PASS" } else { "FAIL" };
            println!("{status} {} ({}/{})", r.name, r.passed, r.total);
            for failure in &r.failures {
                println!("{}\n", indent(failure));
            }
        }
    }
    println!(
        "\n{engine}: {passed}/{total} records passed, {files_passed}/{} files fully passing",
        results.len()
    );

    if args.postgis {
        // Against the oracle, every record should pass. Anything else means the expectations are
        // stale (or PostGIS versions differ).
        return if passed == total {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }

    let mut parity = read_parity();
    if !cfg!(feature = "geos-3_11") {
        println!(
            "\nWARNING: built without --all-features, but parity.txt is recorded with all \
             features (GEOS-backed functions are missing). Use `cargo slt`."
        );
        if args.update_parity {
            return ExitCode::FAILURE;
        }
    }
    if args.update_parity {
        if args.filters.is_empty() {
            parity.clear();
        }
        for r in &results {
            parity.insert(r.name.clone(), (r.passed, r.total));
        }
        write_parity(&parity);
        println!("Updated {}", parity_path().display());
        return ExitCode::SUCCESS;
    }

    let mut regressions = vec![];
    let mut improvements = vec![];
    for r in &results {
        let actual = (r.passed, r.total);
        match parity.get(&r.name) {
            Some(&recorded) if recorded == actual => {}
            Some(&(rp, rt)) if r.passed < rp => {
                regressions.push(format!("{} {rp}/{rt} -> {}/{}", r.name, r.passed, r.total))
            }
            Some(&(rp, rt)) => {
                improvements.push(format!("{} {rp}/{rt} -> {}/{}", r.name, r.passed, r.total))
            }
            None => improvements.push(format!("{} (new) {}/{}", r.name, r.passed, r.total)),
        }
    }
    if args.filters.is_empty() {
        for name in parity.keys() {
            if !results.iter().any(|r| &r.name == name) {
                improvements.push(format!("{name} (file removed)"));
            }
        }
    }
    if !regressions.is_empty() {
        println!("\nREGRESSIONS vs parity.txt (fewer records passing than recorded):");
        for r in &regressions {
            println!("  {r}");
        }
    }
    if !improvements.is_empty() {
        println!("\nChanged vs parity.txt (record with --update-parity):");
        for i in &improvements {
            println!("  {i}");
        }
    }
    if regressions.is_empty() && improvements.is_empty() {
        println!("parity.txt is up to date");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
