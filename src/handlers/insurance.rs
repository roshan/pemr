//! Insurance directory + coverage CRUD. Shared reference data (a family shares
//! one card): plans live at `/insurance`; covered people are linked per-plan.
//! Plain form POST → redirect, matching the providers/sources handlers.

use axum::extract::{Form, Multipart, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::files::{self, StoredFile};
use crate::images;
use crate::handlers::{AppState, load_subjects};
use crate::models::{
    INSURANCE_CARD_SIDES, INSURANCE_PLAN_KINDS, INSURANCE_PLAN_TYPES, INSURANCE_RELATIONSHIPS,
    InsuranceCard, InsurancePlan, Source, SubjectInsurance, empty_to_none, parse_date,
};
use crate::viewer::ViewerContext;
use crate::views::insurance;
use crate::views::layout::Nav;

async fn load_sources(pool: &sqlx::PgPool) -> Result<Vec<Source>, sqlx::Error> {
    sqlx::query_as::<_, Source>("select * from sources order by name")
        .fetch_all(pool)
        .await
}

fn parse_opt_uuid(s: String, field: &str) -> AppResult<Option<Uuid>> {
    match empty_to_none(s) {
        None => Ok(None),
        Some(v) => Uuid::parse_str(&v)
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("invalid {field}"))),
    }
}

pub async fn list(State(state): State<AppState>, viewer: ViewerContext) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plans =
        sqlx::query_as::<_, InsurancePlan>("select * from insurance_plans order by payer_name, plan_name")
            .fetch_all(&state.pool)
            .await?;
    let counts = sqlx::query_as::<_, (Uuid, i64)>(
        "select plan_id, count(*) from subject_insurance group by plan_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Insurance",
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(insurance::list_page(&nav, &plans, &counts, &sources))
}

#[derive(Debug, Deserialize)]
pub struct PlanForm {
    pub payer_name: String,
    #[serde(default)]
    pub plan_name: String,
    #[serde(default)]
    pub plan_kind: String,
    #[serde(default)]
    pub plan_type: String,
    #[serde(default)]
    pub member_id: String,
    #[serde(default)]
    pub group_number: String,
    #[serde(default)]
    pub subscriber_name: String,
    #[serde(default)]
    pub rx_bin: String,
    #[serde(default)]
    pub rx_pcn: String,
    #[serde(default)]
    pub rx_group: String,
    #[serde(default)]
    pub payer_phone: String,
    #[serde(default)]
    pub effective_date: String,
    #[serde(default)]
    pub expiration_date: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub notes: String,
}

/// Validate + normalize the plan-kind / plan-type pair from a form.
fn validate_plan_kinds(kind: &str, plan_type: &str) -> AppResult<(String, Option<String>)> {
    let kind = if kind.trim().is_empty() { "medical" } else { kind.trim() };
    if !INSURANCE_PLAN_KINDS.contains(&kind) {
        return Err(AppError::BadRequest(format!("unknown coverage kind: {kind}")));
    }
    let plan_type = empty_to_none(plan_type.to_string());
    if let Some(t) = &plan_type {
        if !INSURANCE_PLAN_TYPES.contains(&t.as_str()) {
            return Err(AppError::BadRequest(format!("unknown plan type: {t}")));
        }
    }
    Ok((kind.to_string(), plan_type))
}

pub async fn create(State(state): State<AppState>, Form(form): Form<PlanForm>) -> AppResult<Response> {
    let payer_name = form.payer_name.trim().to_string();
    if payer_name.is_empty() {
        return Err(AppError::BadRequest("payer_name required".into()));
    }
    let (plan_kind, plan_type) = validate_plan_kinds(&form.plan_kind, &form.plan_type)?;
    let source_id = parse_opt_uuid(form.source_id, "source")?;
    let effective_date = parse_date(&form.effective_date).map_err(AppError::BadRequest)?;
    let expiration_date = parse_date(&form.expiration_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "insert into insurance_plans (id, payer_name, plan_name, plan_kind, plan_type, member_id,
            group_number, subscriber_name, rx_bin, rx_pcn, rx_group, payer_phone,
            effective_date, expiration_date, source_id, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(Uuid::now_v7())
    .bind(&payer_name)
    .bind(empty_to_none(form.plan_name))
    .bind(&plan_kind)
    .bind(plan_type)
    .bind(empty_to_none(form.member_id))
    .bind(empty_to_none(form.group_number))
    .bind(empty_to_none(form.subscriber_name))
    .bind(empty_to_none(form.rx_bin))
    .bind(empty_to_none(form.rx_pcn))
    .bind(empty_to_none(form.rx_group))
    .bind(empty_to_none(form.payer_phone))
    .bind(effective_date)
    .bind(expiration_date)
    .bind(source_id)
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to("/insurance").into_response())
}

async fn load_plan(pool: &sqlx::PgPool, id: Uuid) -> AppResult<InsurancePlan> {
    sqlx::query_as::<_, InsurancePlan>("select * from insurance_plans where id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plan = load_plan(&state.pool, id).await?;
    let covered = sqlx::query_as::<_, SubjectInsurance>(
        "select * from subject_insurance where plan_id = $1 order by is_primary desc, created_at",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &plan.payer_name,
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    let cards = load_cards(&state.pool, id).await?;
    Ok(insurance::detail_page(&nav, &plan, &covered, &subjects, &cards))
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let plan = load_plan(&state.pool, id).await?;
    let sources = load_sources(&state.pool).await?;
    let nav = Nav {
        title: "Edit insurance plan",
        current_path: "/insurance",
        subjects: &subjects,
        current_subject: None,
        viewer: &viewer,
    };
    Ok(insurance::edit_form(&nav, &plan, &sources, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<PlanForm>,
) -> AppResult<Response> {
    let payer_name = form.payer_name.trim().to_string();
    if payer_name.is_empty() {
        return Err(AppError::BadRequest("payer_name required".into()));
    }
    let (plan_kind, plan_type) = validate_plan_kinds(&form.plan_kind, &form.plan_type)?;
    let source_id = parse_opt_uuid(form.source_id, "source")?;
    let effective_date = parse_date(&form.effective_date).map_err(AppError::BadRequest)?;
    let expiration_date = parse_date(&form.expiration_date).map_err(AppError::BadRequest)?;
    sqlx::query(
        "update insurance_plans set payer_name=$2, plan_name=$3, plan_kind=$4, plan_type=$5,
            member_id=$6, group_number=$7, subscriber_name=$8, rx_bin=$9, rx_pcn=$10, rx_group=$11,
            payer_phone=$12, effective_date=$13, expiration_date=$14, source_id=$15, notes=$16,
            updated_at=now()
          where id=$1",
    )
    .bind(id)
    .bind(&payer_name)
    .bind(empty_to_none(form.plan_name))
    .bind(&plan_kind)
    .bind(plan_type)
    .bind(empty_to_none(form.member_id))
    .bind(empty_to_none(form.group_number))
    .bind(empty_to_none(form.subscriber_name))
    .bind(empty_to_none(form.rx_bin))
    .bind(empty_to_none(form.rx_pcn))
    .bind(empty_to_none(form.rx_group))
    .bind(empty_to_none(form.payer_phone))
    .bind(effective_date)
    .bind(expiration_date)
    .bind(source_id)
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CoverageForm {
    pub subject_id: Uuid,
    pub relationship: String,
    #[serde(default)]
    pub member_id: String,
    #[serde(default)]
    pub is_primary: Option<String>,
}

pub async fn cover_subject(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CoverageForm>,
) -> AppResult<Response> {
    let relationship = form.relationship.trim().to_string();
    if !INSURANCE_RELATIONSHIPS.contains(&relationship.as_str()) {
        return Err(AppError::BadRequest(format!("unknown relationship: {relationship}")));
    }
    let is_primary = form.is_primary.is_some();
    sqlx::query(
        "insert into subject_insurance (subject_id, plan_id, relationship, member_id, is_primary)
         values ($1,$2,$3,$4,$5)
         on conflict (subject_id, plan_id) do update set
            relationship = excluded.relationship, member_id = excluded.member_id,
            is_primary = excluded.is_primary",
    )
    .bind(form.subject_id)
    .bind(id)
    .bind(&relationship)
    .bind(empty_to_none(form.member_id))
    .bind(is_primary)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}

pub async fn uncover_subject(
    State(state): State<AppState>,
    Path((id, subject_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    sqlx::query("delete from subject_insurance where plan_id = $1 and subject_id = $2")
        .bind(id)
        .bind(subject_id)
        .execute(&state.pool)
        .await?;
    Ok(Redirect::to(&format!("/insurance/{id}")).into_response())
}

// ---------------------------------------------------------------------------
// Card images
//
// A card is bytes, so it follows the records/DICOM path exactly: content-
// addressed under FILES_DIR + a webp thumbnail. The one extra rule is that a
// stored card is ALWAYS decodable image bytes -- we sniff the magic bytes and
// reject anything else rather than trusting the client's content-type. That is
// what lets `GET /api/v1/insurance-plans/{id}/card` promise an image to an
// agent that just wants to display it (PEMR-51).
// ---------------------------------------------------------------------------

/// Cards are photos or scans, not imaging studies -- a couple of MB at most.
/// Deliberately far below the records ceiling so a misdirected upload fails
/// fast instead of filling the volume.
const MAX_CARD_BYTES: usize = 32 * 1024 * 1024;

pub async fn upload_card(
    State(state): State<AppState>,
    Path(plan_id): Path<Uuid>,
    mut multipart: Multipart,
) -> AppResult<Response> {
    let mut side = "front".to_string();
    let mut effective_date = String::new();
    let mut notes = String::new();
    let mut file_bytes: Option<bytes::Bytes> = None;

    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or("") {
            "side" => side = field.text().await?,
            "effective_date" => effective_date = field.text().await?,
            "notes" => notes = field.text().await?,
            "file" => {
                let bytes = field.bytes().await?;
                if bytes.len() > MAX_CARD_BYTES {
                    return Err(AppError::BadRequest(format!(
                        "card image too large: {} bytes (max {MAX_CARD_BYTES})",
                        bytes.len()
                    )));
                }
                if !bytes.is_empty() {
                    file_bytes = Some(bytes);
                }
            }
            _ => {
                let _ = field.bytes().await?;
            }
        }
    }

    let side = side.trim().to_lowercase();
    if !INSURANCE_CARD_SIDES.contains(&side.as_str()) {
        return Err(AppError::BadRequest(format!("unknown card side: {side}")));
    }
    let effective_date = parse_date(&effective_date).map_err(AppError::BadRequest)?;
    let bytes = file_bytes.ok_or_else(|| AppError::BadRequest("no image uploaded".into()))?;

    // Reject non-images up front: the retrieval contract is "you get an image".
    // A PDF card has to be rasterised before upload, and we say so.
    let mime = images::sniff_mime(&bytes).ok_or_else(|| {
        AppError::BadRequest(
            "card must be an image (PNG, JPEG, WebP or GIF). PDFs and TIFFs need converting first."
                .into(),
        )
    })?;
    let ext = images::sniff_extension(&bytes).unwrap_or("bin");

    let stored: StoredFile = files::store_bytes(&state.files_dir, &bytes, Some(ext)).await?;
    let thumb: Option<StoredFile> = match images::thumbnail_webp(&bytes, 400) {
        Ok(webp) => Some(files::store_bytes(&state.files_dir, &webp, Some("webp")).await?),
        Err(e) => {
            tracing::warn!(error = %e, "card thumbnail failed; storing without one");
            None
        }
    };

    // Replacing a side retires the old card rather than deleting it: a lapsed
    // card is still the right evidence for a claim from that plan year. The
    // partial unique index requires this to happen in the same transaction.
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "update insurance_cards set superseded_at = now(), updated_at = now()
          where plan_id = $1 and side = $2 and superseded_at is null",
    )
    .bind(plan_id)
    .bind(&side)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into insurance_cards
            (id, plan_id, side, file_path, content_type, byte_size, sha256,
             thumbnail_path, thumbnail_content_type, effective_date, notes)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(Uuid::now_v7())
    .bind(plan_id)
    .bind(&side)
    .bind(&stored.relative_path)
    .bind(mime)
    .bind(stored.byte_size)
    .bind(&stored.sha256_hex)
    .bind(thumb.as_ref().map(|t| t.relative_path.clone()))
    .bind(thumb.as_ref().map(|_| "image/webp"))
    .bind(effective_date)
    .bind(notes)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Redirect::to(&format!("/insurance/{plan_id}")).into_response())
}

/// Retire a card without uploading a replacement (plan cancelled, card lost).
pub async fn supersede_card(
    State(state): State<AppState>,
    Path((plan_id, card_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    sqlx::query(
        "update insurance_cards set superseded_at = now(), updated_at = now()
          where id = $1 and plan_id = $2 and superseded_at is null",
    )
    .bind(card_id)
    .bind(plan_id)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/insurance/{plan_id}")).into_response())
}

pub async fn card_file(
    State(state): State<AppState>,
    Path(card_id): Path<Uuid>,
) -> AppResult<Response> {
    let row: Option<(String, Option<String>, Option<i64>, String)> = sqlx::query_as(
        "select c.file_path, c.content_type, c.byte_size, p.payer_name
           from insurance_cards c join insurance_plans p on p.id = c.plan_id
          where c.id = $1",
    )
    .bind(card_id)
    .fetch_optional(&state.pool)
    .await?;
    let (path, ct, size, payer) = row.ok_or(AppError::NotFound)?;
    crate::handlers::records::serve_file(&state, Some(path), ct, size, &payer).await
}

pub async fn card_thumbnail(
    State(state): State<AppState>,
    Path(card_id): Path<Uuid>,
) -> AppResult<Response> {
    let row: Option<(Option<String>, Option<String>, String)> = sqlx::query_as(
        "select c.thumbnail_path, c.thumbnail_content_type, p.payer_name
           from insurance_cards c join insurance_plans p on p.id = c.plan_id
          where c.id = $1",
    )
    .bind(card_id)
    .fetch_optional(&state.pool)
    .await?;
    let (path, ct, payer) = row.ok_or(AppError::NotFound)?;
    crate::handlers::records::serve_file(&state, path, ct, None, &payer).await
}

/// Current cards for a plan, front first — what the detail page renders.
pub async fn load_cards(pool: &sqlx::PgPool, plan_id: Uuid) -> Result<Vec<InsuranceCard>, sqlx::Error> {
    sqlx::query_as::<_, InsuranceCard>(
        "select * from insurance_cards where plan_id = $1 and superseded_at is null
          order by case side when 'front' then 0 else 1 end",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await
}

/// One tile on the home dashboard's "Insurance cards" lane (PEMR-55 Part C):
/// a plan plus its current front card, if any. The card image is the artifact
/// you reach from the dashboard in one tap.
#[derive(Debug, Clone)]
pub struct InsuranceCardTile {
    pub plan: InsurancePlan,
    pub current_front: Option<InsuranceCard>,
}

/// All plans with coverage, each with its current front card (if on file) —
/// ordered by payer, primary-coverage plans first. `None` front = clean
/// "no card on file" absence, matching the API's 404-as-absence contract.
pub async fn load_dashboard_tiles(
    pool: &sqlx::PgPool,
    subject: Option<Uuid>,
) -> Result<Vec<InsuranceCardTile>, sqlx::Error> {
    let plans: Vec<InsurancePlan> = match subject {
        Some(sid) => sqlx::query_as::<_, InsurancePlan>(
            "select p.* from insurance_plans p
               join subject_insurance si on si.plan_id = p.id
              where si.subject_id = $1
              order by si.is_primary desc, p.payer_name",
        )
        .bind(sid)
        .fetch_all(pool)
        .await?,
        None => sqlx::query_as::<_, InsurancePlan>(
            "select * from insurance_plans order by payer_name",
        )
        .fetch_all(pool)
        .await?,
    };
    let mut tiles = Vec::with_capacity(plans.len());
    for p in plans {
        let front = sqlx::query_as::<_, InsuranceCard>(
            "select * from insurance_cards
              where plan_id = $1 and side = 'front' and superseded_at is null
              order by created_at desc limit 1",
        )
        .bind(p.id)
        .fetch_optional(pool)
        .await?;
        tiles.push(InsuranceCardTile { plan: p, current_front: front });
    }
    Ok(tiles)
}
