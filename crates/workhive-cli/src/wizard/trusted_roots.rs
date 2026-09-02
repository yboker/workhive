//! Trusted project roots step for global init.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// Trusted project roots step (global init)
// ══════════════════════════════════════════════════════════════════════════════

/// Pure decision step for the trusted-roots wizard loop.
///
/// Extracted from [`ask_trusted_roots`] so the policy for each
/// `PathInputOutcome` × `roots.is_empty()` combination can be
/// exercised without spinning up a real terminal session. The
/// `BackToAddMoreDecision` variant captures the contract that once
/// at least one trusted root has been accepted, `Esc` in the
/// path-input screen must NOT loop back into the same picker — it
/// must return the user to the explicit
/// `Add another trusted project directory?` decision point so an
/// accidental "yes" is reversible without losing accepted state.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TrustedRootsLoopStep {
    /// User submitted a non-empty path; the caller must validate it.
    SubmitCandidate(String),
    /// Required-state guard: keep the loop running and remind the user
    /// that at least one trusted root must be accepted before the
    /// global-init step can complete.
    RemindAtLeastOne,
    /// User pressed Esc with at least one trusted root already accepted.
    /// The caller must return to the explicit "add another?"
    /// confirmation (default No) so accepted roots and prior wizard
    /// choices are preserved.
    BackToAddMoreDecision,
    /// User signalled "done adding"; the caller breaks out of the loop
    /// and proceeds with the accepted trusted-root list.
    Finish,
}

/// Map a [`PathInputOutcome`] plus the current `roots.is_empty()` state
/// to the next [`TrustedRootsLoopStep`] the wizard must execute.
///
/// The matrix:
///
/// | outcome / state    | roots empty           | roots non-empty           |
/// | ------------------ | --------------------- | ------------------------- |
/// | `Submitted(s)`     | `SubmitCandidate(s)`  | `SubmitCandidate(s)`      |
/// | `EmptyEnter`       | `RemindAtLeastOne`    | `Finish`                  |
/// | `Cancelled`        | `RemindAtLeastOne`    | `BackToAddMoreDecision`   |
///
/// `RemindAtLeastOne` is used for both Cancelled and EmptyEnter while
/// no roots are accepted because global init requires at least one
/// trusted root before this step can finish.
pub(crate) fn next_trusted_roots_step(
    outcome: crate::ui::screens::path_input::PathInputOutcome,
    roots_is_empty: bool,
) -> TrustedRootsLoopStep {
    use crate::ui::screens::path_input::PathInputOutcome as P;
    match outcome {
        P::Submitted(value) => TrustedRootsLoopStep::SubmitCandidate(value),
        P::EmptyEnter => {
            if roots_is_empty {
                TrustedRootsLoopStep::RemindAtLeastOne
            } else {
                TrustedRootsLoopStep::Finish
            }
        }
        P::Cancelled => {
            if roots_is_empty {
                TrustedRootsLoopStep::RemindAtLeastOne
            } else {
                TrustedRootsLoopStep::BackToAddMoreDecision
            }
        }
    }
}

/// Prompt the user for one or more trusted project roots.
///
/// Entry rules:
///
/// 1. Ask an explicit Yes/No before showing the path prompt.
/// 2. Selecting `No` bounces back to the same Yes/No question with a
///    localized warning. Empty trusted-root list is never an
///    accepted final state for global init.
/// 3. Path entry uses the dedicated `path_input` UI component so
///    `Tab` triggers real directory completion via the
///    `PathCompletionFsPort`.
/// 4. Each candidate is validated through
///    `classify_trusted_root_input` (shape check, existence,
///    directory check, duplicate detection) — the wizard does not
///    re-implement the policy.
/// 5. Submission is only allowed once at least one trusted root has
///    been accepted.
pub(super) fn ask_trusted_roots(
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<Vec<Utf8PathBuf>, CliError> {
    use workhive_core::usecases::trusted_roots::{
        TrustedRootValidation, classify_trusted_root_input_with_strategy,
    };
    use workhive_infra::path_completion::FsPathCompletion;

    eprintln!("── {} ──", i18n.resolve(locale, k::INIT_STEP_TRUSTED_ROOTS));
    eprintln!();
    eprintln!("  {}", i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_DESC));
    eprintln!();

    let home = home_dir_utf8();
    let fs = FsPathCompletion::new();
    let mut roots: Vec<Utf8PathBuf> = Vec::new();
    // Authored display form (e.g. `~/development`) for every accepted
    // root, captured *in order of acceptance*. Surfaced inside the
    // path-input screen so the user always sees the running list
    // without scrolling terminal scrollback.
    let mut added_authored: Vec<String> = Vec::new();
    // Retry state for the path-entry loop. After a rejected
    // submission it preserves the rejected text as the next prompt's
    // initial buffer (so the user can edit or Tab-complete it instead
    // of retyping) and carries the pre-localized blocking feedback
    // threaded into `PathInputOptions::validation_error` so the
    // failure renders in-frame with `Danger` styling. Cleared on the
    // next accepted submission.
    let mut input_retry = ValidatedInputRetry::new();

    // Yes/No entry gate. We loop here because the doc requires that
    // declining must NOT silently complete with an empty list — we
    // re-prompt with a clear localized notice instead.
    loop {
        let entry_prompt = i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_ENTRY_ASK);
        let proceed = confirm_with_transcript(i18n, locale, &entry_prompt, true)?;
        if proceed {
            break;
        }
        eprintln!(
            "  {}",
            i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_DECLINE_REPROMPT),
        );
        eprintln!();
    }

    // Path entry loop. Continues until the user has at least one
    // valid trusted root AND chooses not to add another.
    //
    // The first prompt is intentionally NOT prefilled with `~/`: a
    // bare-home suggestion is always rejected by core as "too broad".
    // The prompt starts empty, and a localized helper line above the
    // input tells the user which shape of path is actually accepted.
    if roots.is_empty() {
        eprintln!(
            "  {}",
            i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_EXAMPLES_HINT),
        );
        eprintln!();
    }
    loop {
        let prompt = i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_PROMPT);
        // Footer copy is gated on whether any roots have already been
        // accepted. Without roots, the loop is in its required state
        // (the user must add at least one), so the existing
        // `Esc: cancel` semantics still apply. Once a root is
        // accepted, `Esc` is rerouted by `next_trusted_roots_step` to
        // the explicit `Add another trusted project directory?`
        // confirmation, and the footer must say `Esc: back` plus
        // surface the `empty Enter: done` finish shortcut so it stops
        // being the only discoverable exit.
        let footer_hint = if roots.is_empty() {
            i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_FOOTER_HINT)
        } else {
            i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_FOOTER_HINT_WITH_ROOTS)
        };
        let no_match_hint = i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_NO_MATCH);
        let field_label = i18n.resolve(locale, k::PATH_INPUT_FIELD_LABEL);
        let placeholder = i18n.resolve(locale, k::PATH_INPUT_PLACEHOLDER_HOME);
        let scope_label_template = i18n.resolve(locale, k::PATH_INPUT_SCOPE_LABEL);
        let preview_label_template = i18n.resolve(locale, k::PATH_INPUT_PREVIEW_LABEL);
        let bare_scope_warning = i18n.resolve(locale, k::PATH_INPUT_PREVIEW_BARE_SCOPE_WARNING);
        let relative_warning = i18n.resolve(locale, k::PATH_INPUT_RELATIVE_WARNING);
        let scope_no_match_warning = i18n.resolve(locale, k::PATH_INPUT_SCOPE_NO_MATCH_WARNING);
        let added_header_template = i18n.resolve(locale, k::PATH_INPUT_ADDED_ROOTS_HEADER);
        let added_empty_label = i18n.resolve(locale, k::PATH_INPUT_ADDED_ROOTS_EMPTY);
        let added_overflow_template = i18n.resolve(locale, k::PATH_INPUT_ADDED_ROOTS_OVERFLOW);
        let dir_no_children_hint = i18n.resolve(locale, k::PATH_INPUT_DIR_NO_CHILDREN);
        // Production callers route through the platform-aware path
        // strategy so Windows drive/UNC inputs work end-to-end while
        // macOS/Linux continue to
        // share `UnixPathStrategy`; Windows gets `WindowsPathStrategy`.
        let strategy = workhive_core::usecases::path_strategy::current_target_strategy();
        let added_summary = crate::ui::screens::path_input::AddedRootsSummary {
            header_template: &added_header_template,
            empty_label: &added_empty_label,
            overflow_template: &added_overflow_template,
            paths: &added_authored,
            // `most_recent` is no longer surfaced visually: every
            // accepted root carries the same valid/success style in
            // the in-screen summary, and per-root acceptance is
            // confirmed by the dedicated "added trusted directory"
            // line emitted to stderr. We pass `None` here so the
            // renderer never applies a distinct highlight to one row.
            most_recent: None,
        };
        let outcome = crate::ui::screens::path_input::run(
            crate::ui::screens::path_input::PathInputOptions {
                prompt: &prompt,
                initial: input_retry.initial(),
                footer_hint: &footer_hint,
                no_match_hint: &no_match_hint,
                dir_no_children_hint: &dir_no_children_hint,
                home: home.as_deref(),
                field_label: &field_label,
                placeholder: &placeholder,
                scope_label_template: &scope_label_template,
                preview_label_template: &preview_label_template,
                bare_scope_warning: &bare_scope_warning,
                relative_warning: &relative_warning,
                scope_no_match_warning: &scope_no_match_warning,
                validation_error: input_retry.feedback(),
                added_summary,
            },
            strategy.as_ref(),
            &fs,
        )?;

        let raw = match next_trusted_roots_step(outcome, roots.is_empty()) {
            TrustedRootsLoopStep::SubmitCandidate(value) => value,
            TrustedRootsLoopStep::RemindAtLeastOne => {
                // No accepted roots yet: keep the user on this step
                // with a localized reminder. Empty Enter MUST NOT mean
                // "trust everything", and Esc with an empty list MUST
                // NOT silently abandon the required step.
                eprintln!(
                    "  {}",
                    i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_REQUIRE_AT_LEAST_ONE),
                );
                continue;
            }
            TrustedRootsLoopStep::Finish => {
                // At least one root already accepted — empty entry
                // is the user's "I'm done adding" signal.
                break;
            }
            TrustedRootsLoopStep::BackToAddMoreDecision => {
                // User pressed Esc inside the picker with at least one
                // root already accepted. Surface the explicit
                // `Add another trusted project directory?` confirmation
                // (default No) so an accidental "yes" earlier is
                // reversible without losing accepted state or prior
                // wizard choices, and without forcing the user to
                // discover the empty-Enter finish shortcut on their
                // own. Picking No finishes the step; picking Yes
                // resumes the picker on the next loop iteration.
                let again_prompt = i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_FINISH_PROMPT);
                let again = confirm_with_transcript(i18n, locale, &again_prompt, false)?;
                if !again {
                    break;
                }
                continue;
            }
        };

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if roots.is_empty() {
                eprintln!(
                    "  {}",
                    i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_REQUIRE_AT_LEAST_ONE),
                );
                continue;
            }
            break;
        }

        let candidate = Utf8PathBuf::from(trimmed);
        let validation = classify_trusted_root_input_with_strategy(
            &candidate,
            &roots,
            home.as_deref(),
            strategy.as_ref(),
            &fs,
        );
        match validation {
            TrustedRootValidation::Accepted {
                canonical,
                authored,
            } => {
                // Honor "preserve authored form" (`~/dev` over
                // `/Users/me/dev`) so the resulting global config
                // stays portable across machines.
                added_authored.push(authored.to_string());
                // Accepted-row styling is now uniform across all
                // entries in the in-screen summary, so no
                // per-row "most recent" index is tracked. We still
                // emit a localized semantic confirmation line to
                // stderr below so users running with screen readers
                // — or scrolling back in their terminal — get an
                // unambiguous "this directory is now a trusted
                // root" message without depending on terminal
                // colour.
                input_retry.clear();
                roots.push(authored);
                let canonical_str = canonical.to_string();
                eprintln!(
                    "    {}",
                    i18n.resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_ADDED, &[&canonical_str],),
                );
            }
            other => {
                // All non-Accepted variants are blocking validation
                // failures. We render them with `Danger` severity in
                // the path-input screen (next loop tick) instead of
                // emitting bare `eprintln!` lines that scroll out of
                // view. We also clear the most-recent highlight so
                // the summary block does not point Preview styling
                // at a stale row while the user is reading the
                // error. Preserve the rejected text as the next
                // prompt's initial buffer so the user can edit or
                // Tab-complete it instead of retyping.
                input_retry.reject(
                    trimmed,
                    format_validation_error(i18n, locale, &other, trimmed),
                );
                continue;
            }
        }

        // After at least one accepted root, ask whether to keep
        // going. Default to `No` because the user has already met
        // the "at least one" requirement.
        let again_prompt = i18n.resolve(locale, k::INIT_TRUSTED_ROOTS_FINISH_PROMPT);
        let again = confirm_with_transcript(i18n, locale, &again_prompt, false)?;
        if !again {
            break;
        }
    }

    let count = roots.len().to_string();
    step_feedback(
        i18n,
        locale,
        &i18n.resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_SUMMARY_LIST, &[&count]),
    );

    Ok(roots)
}

pub(super) fn home_dir_utf8() -> Option<Utf8PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(Utf8PathBuf::from)
}

/// Map a non-`Accepted` `TrustedRootValidation` to a single
/// pre-localized blocking-error string for the path-input screen.
///
/// The path-input renderer always applies `MessageSeverity::Danger`
/// to this string, so the *severity decision is anchored in the
/// enum variant*, not in text matching. Adding a future
/// `TrustedRootValidation` variant only requires extending this
/// mapper.
///
/// The caller-supplied prefix (`PATH_INPUT_VALIDATION_PREFIX`) is
/// prepended once so the in-frame error reads as a uniform
/// `<prefix> <details>` line regardless of which validation failure
/// fired. `raw_input` is the trimmed user-typed buffer; it is only
/// substituted into the `NotAbsoluteOrTilde` template (which is
/// shape-driven and therefore has no `canonical` form to display).
pub(crate) fn format_validation_error(
    i18n: &StaticI18n,
    locale: Locale,
    validation: &workhive_core::usecases::trusted_roots::TrustedRootValidation,
    raw_input: &str,
) -> String {
    use workhive_core::usecases::trusted_roots::TrustedRootValidation as V;
    let prefix = i18n.resolve(locale, k::PATH_INPUT_VALIDATION_PREFIX);
    let body = match validation {
        V::Accepted { .. } => {
            // Defensive: callers must only invoke this mapper on
            // non-Accepted variants, but we still produce a stable
            // string so an accidental call cannot panic.
            String::new()
        }
        V::NotAbsoluteOrTilde => {
            i18n.resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_INVALID, &[raw_input])
        }
        V::TooBroad { canonical } => i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_REJECT_OVERLY_BROAD,
            &[canonical.as_str()],
        ),
        V::DoesNotExist { canonical } => i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_PATH_MISSING,
            &[canonical.as_str()],
        ),
        V::NotADirectory { canonical } => i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_PATH_NOT_DIR,
            &[canonical.as_str()],
        ),
        V::Duplicate { canonical } => i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_DUPLICATE,
            &[canonical.as_str()],
        ),
        V::CoveredByExistingRoot {
            canonical,
            covering_root,
        } => i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_COVERED_BY_EXISTING_ROOT,
            &[canonical.as_str(), covering_root.as_str()],
        ),
    };
    if body.is_empty() {
        prefix
    } else {
        format!("{} {}", prefix, body)
    }
}
