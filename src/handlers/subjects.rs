use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use maud::Markup;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::handlers::{AppState, load_subjects};
use crate::models::{
    Allergy, Appointment, CareTeamMember, Condition, Immunization, Incident, InsuranceCoverageRow,
    Medication, Record, Subject, VitalRow, empty_to_none, parse_date,
};
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
    pub cf_access_email: String,
    #[serde(default)]
    pub notes: String,
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
    let id = Uuid::now_v7();
    let full_name = format!("{given_name} {family_name}");
    sqlx::query(
        "insert into subjects (id, full_name, given_name, family_name, dob,
                               sex_at_birth, blood_type, notes, cf_access_email)
         values ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(&full_name)
    .bind(&given_name)
    .bind(&family_name)
    .bind(dob)
    .bind(empty_to_none(form.sex_at_birth))
    .bind(empty_to_none(form.blood_type))
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

    let summary = clinical_summary_for(&state.pool, &s).await?;
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
    Ok(subject::dashboard_page(&nav, &s, &summary, &data, &timeline))
}

/// Loads the clinical summary (Phase 1/2 tables) shared by the subject chart,
/// the printable summary, and the home-dashboard clinical snapshot.
pub(crate) async fn clinical_summary_for(
    pool: &sqlx::PgPool,
    s: &Subject,
) -> Result<subject::ClinicalSummary, sqlx::Error> {
    let allergies = sqlx::query_as::<_, Allergy>(
        "select * from allergies where subject_id = $1 and status <> 'entered_in_error'
          order by created_at desc",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let medications = sqlx::query_as::<_, Medication>(
        "select * from medications where subject_id = $1 and status = 'active'
          order by created_at desc",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let conditions = sqlx::query_as::<_, Condition>(
        "select * from conditions where subject_id = $1 and status = 'active'
          order by onset_date desc nulls last, created_at desc",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let immunizations = sqlx::query_as::<_, Immunization>(
        "select * from immunizations where subject_id = $1
          order by occurred_at desc nulls last, created_at desc",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let vaccines_due = match s.dob {
        Some(dob) => crate::peds::forecast(dob, &immunizations, crate::peds::today())
            .iter()
            .filter(|d| d.status != "upcoming")
            .count(),
        None => 0,
    };
    let vitals = sqlx::query_as::<_, VitalRow>(
        "select display, value_num::float8 as value_num, value_text, unit, effective_on, abnormal_flag
           from observations where subject_id = $1
          order by effective_on desc, created_at desc limit 8",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let upcoming_appts = sqlx::query_as::<_, Appointment>(
        "select * from appointments where subject_id = $1 and starts_at >= now()
          order by starts_at asc limit 6",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let care_team = sqlx::query_as::<_, CareTeamMember>(
        "select sp.role, p.full_name, p.specialty
           from subject_providers sp join providers p on p.id = sp.provider_id
          where sp.subject_id = $1 and sp.active
          order by p.full_name",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    let insurance = sqlx::query_as::<_, InsuranceCoverageRow>(
        "select p.payer_name, p.plan_name, p.plan_kind,
                si.relationship, coalesce(si.member_id, p.member_id) as member_id
           from subject_insurance si join insurance_plans p on p.id = si.plan_id
          where si.subject_id = $1
          order by si.is_primary desc, p.payer_name",
    )
    .bind(s.id)
    .fetch_all(pool)
    .await?;
    Ok(subject::ClinicalSummary {
        subject_id: s.id,
        no_known_allergies: s.no_known_allergies,
        allergies,
        medications,
        conditions,
        immunizations,
        vitals,
        upcoming_appts,
        care_team,
        insurance,
        vaccines_due,
    })
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
    let cs = clinical_summary_for(&state.pool, &s).await?;
    let nav = Nav {
        title: &s.full_name,
        current_path: "/subjects",
        subjects: &subjects,
        current_subject: Some(id),
        viewer: &viewer,
    };
    Ok(crate::views::summary::page(&nav, &s, &cs))
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

/// `/subjects/{id}/growth` — growth trend charts (PEMR-24).
pub async fn growth(
    State(state): State<AppState>,
    viewer: ViewerContext,
    Path(id): Path<Uuid>,
) -> AppResult<Markup> {
    use crate::growth_ref::{self, Measure};
    use crate::views::growth::GrowthSeries;
    let subjects = load_subjects(&state.pool).await?;
    let s = sqlx::query_as::<_, Subject>("select * from subjects where id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

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
    if let Some(dob) = s.dob {
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
            let points = to_age(raw(&state.pool, id, code).await?);
            let reference = match sex {
                Some(sx) => growth_ref::curve(measure, sx),
                None => Vec::new(),
            };
            series.push(GrowthSeries { label, unit, points, reference });
        }
    }

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
    let full_name = format!("{given_name} {family_name}");
    sqlx::query(
        "update subjects set
            given_name = $2,
            family_name = $3,
            full_name = $4,
            dob = $5,
            sex_at_birth = $6,
            blood_type = $7,
            cf_access_email = $8,
            notes = $9,
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
    .bind(empty_to_none(form.cf_access_email))
    .bind(form.notes)
    .execute(&state.pool)
    .await?;
    Ok(Redirect::to(&format!("/subjects/{id}")).into_response())
}
