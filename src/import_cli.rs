//! Offline bulk importer: `personal-emr import <path> [flags]`.
//!
//! Reads a MyChart / Epic **IHE_XDM zip**, a single **C-CDA `.xml`**, a
//! directory of either, or a **FHIR `.json`** bundle from disk and upserts the
//! clinical rows straight into the database named by `DATABASE_URL` — no running
//! server, no API bearer token, no Cloudflare in the path. Dry-run by default;
//! pass `--commit` to write.
//!
//! ```text
//! personal-emr import ~/Downloads/HealthSummary.zip --source "Sutter Health"
//! personal-emr import ~/Downloads/HealthSummary.zip --subject "Julie" --commit
//! ```

use std::error::Error;
use std::io::Read;
use std::path::Path;

use sqlx::PgPool;
use uuid::Uuid;

use crate::importer;

type R<T> = Result<T, Box<dyn Error>>;

#[derive(PartialEq, Clone, Copy)]
enum DocKind {
    Ccda,
    Fhir,
}

struct Doc {
    name: String,
    kind: DocKind,
    text: String,
}

struct Args {
    path: String,
    subject: Option<String>,
    source: String,
    commit: bool,
}

const USAGE: &str =
    "usage: personal-emr import <zip|xml|json|dir> [--subject <uuid|name>] [--source <name>] [--commit]";

pub async fn run(argv: &[String]) -> R<()> {
    let args = parse_args(argv)?;

    let docs = load_documents(&args.path)?;
    if docs.is_empty() {
        return Err(format!("no C-CDA (.xml) or FHIR (.json) documents found at {}", args.path).into());
    }
    let ccda: Vec<&Doc> = docs.iter().filter(|d| d.kind == DocKind::Ccda).collect();
    let fhir: Vec<&Doc> = docs.iter().filter(|d| d.kind == DocKind::Fhir).collect();
    eprintln!("found {} C-CDA + {} FHIR document(s)", ccda.len(), fhir.len());

    let ccda_xml: Vec<String> = ccda.iter().map(|d| d.text.clone()).collect();

    // Parse-only preview — safe, no DB connection needed.
    let preview = importer::preview_ccda_docs(&ccda_xml);
    print_preview(&preview);

    if !args.commit {
        eprintln!("\nDRY RUN — nothing written. Re-run with --commit to import.");
        return Ok(());
    }

    // DB is only needed to write — keep dry-run usable without one configured.
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is required for --commit")?;
    let pool = crate::db::connect(&database_url).await?;
    let subject_id = resolve_subject(&pool, &args, &ccda_xml).await?;
    let source_id = ensure_source(&pool, &args.source).await?;
    eprintln!("\nimporting → subject {subject_id}, source '{}'", args.source);

    let mut total = importer::import_ccda_docs(&pool, subject_id, source_id, &ccda_xml).await?;
    for d in &fhir {
        eprintln!("  fhir: {}", d.name);
        let bundle: serde_json::Value = serde_json::from_str(&d.text)?;
        let c = importer::import_fhir(&pool, subject_id, source_id, &bundle).await?;
        total.allergies += c.allergies;
        total.medications += c.medications;
        total.conditions += c.conditions;
        total.incidents += c.incidents;
        total.immunizations += c.immunizations;
        total.observations += c.observations;
        total.skipped += c.skipped;
    }
    eprintln!(
        "\n✓ imported: {} allergies · {} meds · {} conditions · {} incidents · {} immunizations · {} observations",
        total.allergies, total.medications, total.conditions, total.incidents, total.immunizations, total.observations
    );
    if total.warnings.is_empty() {
        eprintln!("  fidelity: clean (no warnings)");
    } else {
        eprintln!("  ⚠ fidelity warnings:");
        for w in &total.warnings {
            eprintln!("    - {w}");
        }
    }
    Ok(())
}

fn parse_args(argv: &[String]) -> R<Args> {
    let mut path: Option<String> = None;
    let mut subject = None;
    let mut source = None;
    let mut commit = false;
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--subject" => subject = Some(it.next().ok_or("--subject needs a value")?.clone()),
            "--source" => source = Some(it.next().ok_or("--source needs a value")?.clone()),
            "--commit" => commit = true,
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with("--") => return Err(format!("unknown flag {s}\n{USAGE}").into()),
            s if path.is_none() => path = Some(s.to_string()),
            _ => return Err(format!("unexpected extra argument\n{USAGE}").into()),
        }
    }
    Ok(Args {
        path: path.ok_or(USAGE)?,
        subject,
        source: source.unwrap_or_else(|| "MyChart import".into()),
        commit,
    })
}

fn classify(name: &str, text: &str) -> Option<DocKind> {
    let lower = name.to_lowercase();
    if lower.ends_with(".xml") && text.contains("ClinicalDocument") {
        Some(DocKind::Ccda)
    } else if lower.ends_with(".json") && text.contains("\"resourceType\"") {
        Some(DocKind::Fhir)
    } else {
        None
    }
}

fn load_documents(path: &str) -> R<Vec<Doc>> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| format!("{path}: {e}"))?;
    let mut docs = Vec::new();
    if meta.is_dir() {
        collect_dir(p, &mut docs)?;
        return Ok(docs);
    }
    let bytes = std::fs::read(p)?;
    if bytes.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            if !f.is_file() {
                continue;
            }
            let name = f.name().to_string();
            let lower = name.to_lowercase();
            if !(lower.ends_with(".xml") || lower.ends_with(".json")) {
                continue;
            }
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                if let Some(kind) = classify(&name, &buf) {
                    docs.push(Doc { name, kind, text: buf });
                }
            }
        }
    } else {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("doc").to_string();
        if let Some(kind) = classify(&name, &text) {
            docs.push(Doc { name, kind, text });
        }
    }
    Ok(docs)
}

fn collect_dir(dir: &Path, docs: &mut Vec<Doc>) -> R<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_dir(&path, docs)?;
            continue;
        }
        let lower = path.to_string_lossy().to_lowercase();
        if !(lower.ends_with(".xml") || lower.ends_with(".json")) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("doc").to_string();
            if let Some(kind) = classify(&name, &text) {
                docs.push(Doc { name, kind, text });
            }
        }
    }
    Ok(())
}

fn print_preview(p: &importer::Preview) {
    eprintln!("\n── extraction preview ─────────────────────────");
    eprintln!("  allergies      {}", p.counts.allergies);
    eprintln!("  medications    {}", p.counts.medications);
    eprintln!("  conditions     {}", p.counts.conditions);
    eprintln!("  incidents      {}", p.counts.incidents);
    eprintln!("  immunizations  {}", p.counts.immunizations);
    eprintln!("  observations   {}  ({} labs, {} vitals)", p.counts.observations, p.labs, p.vitals);
    eprintln!("───────────────────────────────────────────────");
    for s in p.samples.iter().take(40) {
        eprintln!("  {s}");
    }
    if p.samples.len() > 40 {
        eprintln!("  … plus more");
    }
    if !p.warnings.is_empty() {
        eprintln!("\n  ⚠ fidelity warnings:");
        for w in &p.warnings {
            eprintln!("    - {w}");
        }
    }
}

async fn resolve_subject(pool: &PgPool, args: &Args, ccda_xml: &[String]) -> R<Uuid> {
    // 1. Explicit --subject: a uuid, or a name fragment matched against subjects.
    if let Some(sel) = &args.subject {
        if let Ok(id) = Uuid::parse_str(sel.trim()) {
            return Ok(id);
        }
        let rows = sqlx::query_as::<_, (Uuid, String)>(
            "select id, full_name from subjects \
             where full_name ilike $1 or given_name ilike $1 or family_name ilike $1 \
             order by family_name, given_name",
        )
        .bind(format!("%{}%", sel.trim()))
        .fetch_all(pool)
        .await?;
        return pick_one(rows, sel);
    }

    // 2. Auto-detect from the document's recordTarget patient name.
    if let Some(xml) = ccda_xml.first() {
        if let Some((given, family)) = importer::ccda_patient_name(xml) {
            eprintln!("document patient: {given} {family}");
            let exact = sqlx::query_as::<_, (Uuid, String)>(
                "select id, full_name from subjects \
                 where lower(given_name) = lower($1) and lower(family_name) = lower($2)",
            )
            .bind(&given)
            .bind(&family)
            .fetch_all(pool)
            .await?;
            if exact.len() == 1 {
                return Ok(exact[0].0);
            }
            let by_family = sqlx::query_as::<_, (Uuid, String)>(
                "select id, full_name from subjects where lower(family_name) = lower($1) \
                 order by family_name, given_name",
            )
            .bind(&family)
            .fetch_all(pool)
            .await?;
            return pick_one(by_family, &format!("{given} {family}"));
        }
    }
    Err("could not determine subject from the document; pass --subject <uuid|name>".into())
}

fn pick_one(rows: Vec<(Uuid, String)>, sel: &str) -> R<Uuid> {
    match rows.len() {
        1 => Ok(rows[0].0),
        0 => Err(format!("no subject matched '{sel}' — pass --subject <uuid>").into()),
        _ => {
            let list = rows
                .iter()
                .map(|(id, n)| format!("  {id}  {n}"))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!("'{sel}' matched {} subjects; pass --subject <uuid>:\n{list}", rows.len()).into())
        }
    }
}

async fn ensure_source(pool: &PgPool, name: &str) -> R<Uuid> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("select id from sources where lower(name) = lower($1)")
            .bind(name)
            .fetch_optional(pool)
            .await?
    {
        return Ok(id);
    }
    let id = Uuid::now_v7();
    sqlx::query("insert into sources (id, name, kind, notes) values ($1,$2,'other',$3)")
        .bind(id)
        .bind(name)
        .bind("Auto-created by offline import.")
        .execute(pool)
        .await?;
    Ok(id)
}
