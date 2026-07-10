use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{Immunization, Incident, Record, Subject, empty_to_none, parse_date};
use crate::viewer::ViewerContext;
use crate::views::layout::Nav;
use crate::views::subject;

pub async fn list(
    State(state): State<AppState>,
    viewer: ViewerContext,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let counts = sqlx::query_as::<_, (Uuid, i64, i64)>(
        "select s.id,
                (select count(*) from incidents i where i.subject_id = s.id),
                (select count(*) from records r where r.subject_id = s.id)
           from subjects s",
    )
    .fetch_all(&state.pool)
    .await?;

    let nav = Nav {
        title: "Subjects",
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: viewer.default_subject_id,
        viewer: &viewer,
    };
    Ok(subject::list_page(&nav, &subjects, &counts))
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub given_name: String,
    pub family_name: String,
    #[serde(default)]
    pub dob: String,
    #[serde(default)]
    pub sex_at_birth: String,
    #[serde(default)]
    pub blood_type: String,
    #[serde(default)]
    pub gestational_age_weeks: String,
    #[serde(default)]
    pub cf_access_email: String,
    #[serde(default)]
    pub notes: String,
}

/// Parse the optional "gestational age at birth (weeks)" input. Blank → None
/// (treated as term). A value outside a plausible range is rejected so a typo
/// can't silently poison the corrected-age computation.
fn parse_gestational_age(s: &str) -> Result<Option<i16>, AppError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let w: i16 = t
        .parse()
        .map_err(|_| AppError::BadRequest("gestational age must be a whole number of weeks".into()))?;
    if !(20..=45).contains(&w) {
        return Err(AppError::BadRequest(
            "gestational age must be between 20 and 45 weeks".into(),
        ));
    }
    Ok(Some(w))
}

pub async fn create(
    State(state): State<AppState>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let given_name = form.given_name.trim().to_string();
    let family_name = form.family_name.trim().to_string();
    if given_name.is_empty() || family_name.is_empty() {
        return Err(AppError::BadRequest(
            "given_name and family_name required".into(),
        ));
    }
    let dob = parse_date(&form.dob).map_err(AppError::BadRequest)?;
    let gestational_age_weeks = parse_gestational_age(&form.gestational_age_weeks)?;
    let id = Uuid::now_v7();
    let full_name = format!("{given_name} {family_name}");
    sqlx::query(
        "insert into subjects (id, full_name, given_name, family_name, dob,
                               sex_at_birth, blood_type, gestational_age_weeks, notes, cf_access_email)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(id)
    .bind(&full_name)
    .bind(&given_name)
    .bind(&family_name)
    .bind(dob)
    .bind(empty_to_none(form.sex_at_birth))
    .bind(empty_to_none(form.blood_type))
    .bind(gestational_age_weeks)
    .bind(form.notes)
    .bind(empty_to_none(form.cf_access_email))
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to("/subjects").into_response())
}

/// `/subjects/{id}` — per-subject dashboard. Bio header on top, then
/// timeline + recent incidents + recent records, all scoped to this subject.
pub async fn detail(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let timeline_limit = crate::views::dashboard::dashboard_timeline_limit() as i64;
    let timeline_incidents = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where subject_id = $1
          order by occurred_at desc nulls last, created_at desc
          limit $2",
    )
    .bind(id)
    .bind(timeline_limit)
    .fetch_all(&state.pool)
    .await?;
    let timeline_total: i64 =
        sqlx::query_scalar("select count(*) from incidents where subject_id = $1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;

    let recent_incidents = sqlx::query_as::<_, Incident>(
        "select id, subject_id, title, narrative, occurred_at, occurred_precision,
                created_at, updated_at
           from incidents
          where subject_id = $1
          order by created_at desc
          limit 10",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let recent_records = sqlx::query_as::<_, Record>(
        "select id, subject_id, kind, title, notes, occurred_at, occurred_precision,
                file_path, content_type, byte_size, sha256,
                preview_path, preview_content_type,
                thumbnail_path, thumbnail_content_type, study_instance_uid,
                dicom_metadata, instance_number,
                source_id, external_id, external_url, source_synced_at,
                created_at, updated_at
           from records
          where subject_id = $1
          order by created_at desc
          limit 10",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let cards =
        crate::subject_modules::render_all(&state.pool, &s, crate::subject_modules::Mode::Card).await?;
    // Opt-in per-subject feature modules (PEMR-45): milestones etc. Absent until
    // the viewer adds them from the chart.
    let feature_area = crate::handlers::milestones::render_feature_area(&state.pool, &s).await?;
    let timeline = crate::handlers::dashboard::load_timeline(&state.pool, Some(id), "1y", None, None).await?;

    let nav = Nav {
        title: &s.full_name,
        current_path: "/",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    let data = crate::views::dashboard::DashboardData {
        subjects: &subjects,
        timeline_incidents: &timeline_incidents,
        timeline_total,
        recent_incidents: &recent_incidents,
        recent_records: &recent_records,
        // The chart renders the full clinical summary below; no snapshot needed.
        clinical: None,
    };
    Ok(subject::dashboard_page(&nav, &s, &cards, &feature_area, &data, &timeline))
}


/// `/subjects/{id}/summary` — print-friendly one-page health summary (PEMR-27).
pub async fn summary(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let sections =
        crate::subject_modules::render_all(&state.pool, &s, crate::subject_modules::Mode::Print).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(crate::views::summary::page(&nav, &s, &sections))
}

/// `/subjects/{id}/vitals` — the full vitals & labs list (all observations).
pub async fn vitals_labs(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let rows = sqlx::query_as::<_, crate::models::ObservationRow>(
        "select effective_on, category, display, value_num::float8 as value_num, value_text, unit,
                ref_low::float8 as ref_low, ref_high::float8 as ref_high, abnormal_flag
           from observations where subject_id = $1
          order by effective_on desc, panel_id nulls first, display",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(crate::views::vitals::page(&nav, &s, &rows))
}

/// `/subjects/{id}/immunizations` — immunization record + forecast (PEMR-25).
pub async fn immunizations(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let received = sqlx::query_as::<_, Immunization>(
        "select * from immunizations where subject_id = $1
          order by occurred_at desc nulls last, created_at desc",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let now = crate::peds::today();
    let (due, well_visits) = match s.dob {
        Some(dob) => (
            crate::peds::forecast(dob, &received, now),
            crate::peds::well_child(dob, now),
        ),
        None => (Vec::new(), Vec::new()),
    };
    // The routine childhood forecast only applies to minors; adults get an
    // explanatory note instead of a misleading "up to date" / "N due".
    let forecast_applicable = s.dob.map(|d| crate::peds::forecast_applies(d, now)).unwrap_or(false);
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(crate::views::immunizations::page(
        &nav,
        &s,
        &received,
        &due,
        &well_visits,
        s.dob.is_some(),
        forecast_applicable,
    ))
}

/// Build weight/length/head-circ growth series for a subject (empty if no DOB).
/// Shared by the full growth page and the subject-chart mini card.
pub(crate) async fn growth_series(
    pool: &sqlx::PgPool,
    s: &Subject,
) -> Result<Vec<crate::views::growth::GrowthSeries>, sqlx::Error> {
    use crate::growth_ref::{self, Measure};
    use crate::views::growth::GrowthSeries;

    async fn raw(
        pool: &sqlx::PgPool,
        subject: Uuid,
        code: &str,
    ) -> Result<Vec<(time::Date, f64)>, sqlx::Error> {
        sqlx::query_as::<_, (time::Date, f64)>(
            "select effective_on, value_num::float8
               from observations
              where subject_id = $1 and code = $2 and value_num is not null
              order by effective_on asc",
        )
        .bind(subject)
        .bind(code)
        .fetch_all(pool)
        .await
    }

    let sex = growth_ref::sex_code(s.sex_at_birth.as_deref());
    let mut series: Vec<GrowthSeries> = Vec::new();
    let Some(dob) = s.dob else { return Ok(series) };
    let dob_jd = dob.to_julian_day();
    let to_age = |rows: Vec<(time::Date, f64)>| -> Vec<(f64, f64)> {
        rows.into_iter()
            .map(|(d, v)| ((d.to_julian_day() - dob_jd) as f64 / 30.4375, v))
            .collect()
    };
    for (label, unit, code, measure) in [
        ("Weight", "kg", "29463-7", Measure::Weight),
        ("Length / height", "cm", "8302-2", Measure::Length),
        ("Head circumference", "cm", "9843-4", Measure::HeadCirc),
    ] {
        let points = to_age(raw(pool, s.id, code).await?);
        let reference = match sex {
            Some(sx) => growth_ref::curve(measure, sx),
            None => Vec::new(),
        };
        series.push(GrowthSeries { label, unit, points, reference });
    }
    Ok(series)
}

/// `/subjects/{id}/growth` — growth trend charts (PEMR-24).
pub async fn growth(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let series = growth_series(&state.pool, &s).await?;
    let sex = crate::growth_ref::sex_code(s.sex_at_birth.as_deref());

    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(crate::views::growth::page(
        &nav,
        &s,
        &series,
        s.dob.is_some(),
        sex.is_some(),
    ))
}

pub async fn edit_form(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let nav = Nav {
        title: "Edit subject",
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(subject::edit_form(&nav, &s, None))
}

pub async fn edit(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateForm>,
) -> AppResult<Response> {
    let given_name = form.given_name.trim().to_string();
    let family_name = form.family_name.trim().to_string();
    if given_name.is_empty() || family_name.is_empty() {
        return Err(AppError::BadRequest(
            "given_name and family_name required".into(),
        ));
    }
    let dob = parse_date(&form.dob).map_err(AppError::BadRequest)?;
    let gestational_age_weeks = parse_gestational_age(&form.gestational_age_weeks)?;
    let full_name = format!("{given_name} {family_name}");
    sqlx::query(
        "update subjects set
            given_name = $2,
            family_name = $3,
            full_name = $4,
            dob = $5,
            sex_at_birth = $6,
            blood_type = $7,
            gestational_age_weeks = $8,
            cf_access_email = $9,
            notes = $10,
            updated_at = now()
          where id = $1",
    )
    .bind(id)
    .bind(&given_name)
    .bind(&family_name)
    .bind(&full_name)
    .bind(dob)
    .bind(empty_to_none(form.sex_at_birth))
    .bind(empty_to_none(form.blood_type))
    .bind(gestational_age_weeks)
    .bind(empty_to_none(form.cf_access_email))
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{id}")).into_response())
}
