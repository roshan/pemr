use maud::{Markup, html};
use uuid::Uuid;

use crate::models::{ApiKey, Subject};
use crate::views::components as c;
use crate::views::layout::{Nav, shell};

pub fn api_keys_page(
    nav: &Nav<'_>,
    keys: &[ApiKey],
    subjects: &[Subject],
    freshly_issued: Option<&str>,
) -> Markup {
    let body = html! {
        (c::page_title("API keys"))
        p class="text-sm text-muted mb-4" {
            "Bearer tokens for the read-only "
            (c::code("/api/v1"))
            " surface. Send each request as "
            (c::code("Authorization: Bearer <token>"))
            ". Keys are issued unscoped — like the UI, every key sees every subject."
        }

        @if let Some(token) = freshly_issued {
            div class="mb-6" {
                (c::card(html! {
                    (c::subheading("Your new API key"))
                    p class="text-sm text-ink mb-2" {
                        "Copy it now. This is the only time it will be shown."
                    }
                    pre class="rounded bg-slate-100 px-3 py-2 text-xs text-ink overflow-x-auto select-all" {
                        (token)
                    }
                }))
            }
        }

        (c::lane(
            html! { (c::section_heading("Existing keys")) },
            html! {
                @if keys.is_empty() {
                    (c::empty_state("No API keys yet."))
                } @else {
                    (c::data_table(
                        html! { tr {
                            (c::th("Name"))
                            (c::th("Prefix"))
                            (c::th("Owner"))
                            (c::th("Last used"))
                            (c::th("Created"))
                            (c::th("Status"))
                            (c::th(""))
                        } },
                        html! {
                            @for k in keys {
                                (key_row(k, subjects))
                            }
                        },
                    ))
                }
            },
        ))

        div class="mt-6" {
            (c::collapse_section("Create a new key", html! {
                (c::form("/settings/api-keys", "post", html! {
                    (c::field_with_hint(
                        "Name",
                        "Where this key will be used (e.g. \"laptop assistant agent\").",
                        c::input_text("name", "", true, Some(120)),
                    ))
                    (c::field_with_hint(
                        "Owner (optional)",
                        "Whose key this is, for revocation tracking. Does not restrict data access.",
                        c::select_field("owner_subject_id", false, || html! {
                            (c::select_option("", "— none —", false))
                            @for s in subjects {
                                (c::select_option(
                                    s.id.to_string(),
                                    format!("{} {}", s.given_name, s.family_name),
                                    false,
                                ))
                            }
                        }),
                    ))
                    (c::button_primary("Generate key"))
                }))
            }, keys.is_empty()))
        }
    };
    shell(nav, body)
}

fn key_row(k: &ApiKey, subjects: &[Subject]) -> Markup {
    let owner = k.owner_subject_id.and_then(|id| subject_name(subjects, id));
    html! {
        tr class="hover:bg-slate-50" {
            (c::td(html! { span class="font-medium" { (k.name) } }))
            (c::td(html! { (c::code(format!("{}…", k.token_prefix))) }))
            (c::td(html! {
                @match owner {
                    Some(name) => (c::badge_subject(name)),
                    None => "—",
                }
            }))
            (c::td(html! { (fmt_timestamp(k.last_used_at)) }))
            (c::td(html! { (fmt_timestamp(Some(k.created_at))) }))
            (c::td(html! {
                @if k.revoked_at.is_some() {
                    (c::badge_neutral("revoked"))
                } @else {
                    span class="text-xs text-muted" { "active" }
                }
            }))
            (c::td(html! {
                @if k.revoked_at.is_none() {
                    (revoke_form(k.id))
                }
            }))
        }
    }
}

fn revoke_form(id: Uuid) -> Markup {
    html! {
        form action={ "/settings/api-keys/" (id) "/revoke" } method="post"
             onsubmit="return confirm('Revoke this key? Apps using it will get 401s immediately.')" {
            button type="submit"
                   class="inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium text-danger hover:bg-rose-50" {
                "Revoke"
            }
        }
    }
}

fn subject_name(subjects: &[Subject], id: Uuid) -> Option<String> {
    subjects
        .iter()
        .find(|s| s.id == id)
        .map(|s| format!("{} {}", s.given_name, s.family_name))
}

fn fmt_timestamp(t: Option<time::OffsetDateTime>) -> String {
    match t {
        None => "—".into(),
        Some(d) => format!(
            "{:04}-{:02}-{:02}",
            d.year(),
            u8::from(d.month()),
            d.day()
        ),
    }
}
