//! Layout planner — converts a `LayoutConfig` DSL into an executable `SplitPlan`.
//!
//! The planner is the bridge between the user-facing layout DSL (which uses
//! "final layout percentages") and the runtime executor (which needs "local
//! split percentages" for each individual split operation).
//!
//! # Architecture
//!
//! ```text
//! LayoutConfig (DSL) ──► plan_layout() ──► SplitPlan ──► Runtime Executor
//! ```
//!
//! # Execution strategy
//!
//! Columns are constructed right-to-left: the last column is split off first
//! from the initial pane, then the second-to-last, and so on. This means
//! each split operates on the remaining space, and we normalize accordingly.
//!
//! Rows within each column follow the same pattern (bottom-to-top): the last
//! pane is split off first from the column's root pane.
//!
//! # Local split percentage formula
//!
//! Given N siblings with final desired ratios `r[0], r[1], ..., r[N-1]`:
//!
//! We split from the last sibling backwards. At each step, processing
//! sibling at index `i`, the local split percentage is:
//!
//! ```text
//! local_pct = r[i] / sum(r[0..=i]) * 100
//! ```
//!
//! where `sum(r[0..=i])` is the sum of ratios for this sibling and all
//! preceding ones (the remaining space at the moment of splitting).
//!
//! The first sibling (index 0) is never split — it occupies whatever remains.

use crate::domain::PaneLaunchRef;
use crate::ports::{LayoutConfig, MAX_TOTAL_PANES};

/// A single split operation in the execution plan.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitStep {
    /// Direction of the split.
    pub direction: SplitDirection,
    /// Local split percentage for the tmux `-p` argument.
    /// This is the percentage of the *current pane* being split, not the
    /// final layout percentage.
    pub local_percent: u8,
    /// Column index in the original DSL (0-based).
    pub col_index: usize,
    /// Pane index within the column (0-based). For column splits, this is
    /// always 0 (the column's root pane).
    pub pane_index: usize,
}

/// Direction of a split operation within the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Horizontal split — creates a new column to the right.
    Horizontal,
    /// Vertical split — creates a new pane below.
    Vertical,
}

/// Structured launch descriptor associated with a specific pane position.
///
/// Replaces the legacy `command: String` field. The `launch` field is the
/// canonical, typed description of what to start in this pane — never a
/// raw shell string, never an empty string sentinel.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneLaunch {
    /// Column index (0-based).
    pub col_index: usize,
    /// Pane index within the column (0-based).
    pub pane_index: usize,
    /// Structured launch descriptor from the DSL.
    pub launch: PaneLaunchRef,
}

/// A fully resolved execution plan for a layout.
///
/// Contains an ordered sequence of split steps and associated launch commands.
/// The runtime executor consumes this plan without needing to understand
/// layout geometry — it just executes splits and sends commands.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitPlan {
    /// Ordered split operations. Executed sequentially.
    /// Column splits come first (right-to-left), then row splits within
    /// each column (bottom-to-top per column, columns processed left-to-right).
    pub steps: Vec<SplitStep>,
    /// Launch commands for each pane, in DSL order (col 0 pane 0, col 0 pane 1, ...).
    pub launches: Vec<PaneLaunch>,
    /// Total number of panes in the final layout.
    pub total_panes: u32,
}

/// Errors from layout planning.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LayoutPlanError {
    #[error("layout has no columns")]
    EmptyColumns,

    #[error("column {col_index} has no panes")]
    EmptyPanes { col_index: usize },

    #[error("column {col_index} has zero width")]
    ZeroWidth { col_index: usize },

    #[error("column {col_index}, pane {pane_index} has zero height")]
    ZeroHeight { col_index: usize, pane_index: usize },

    #[error("total pane count {count} exceeds maximum {max}")]
    TooManyPanes { count: usize, max: usize },
}

/// Convert a `LayoutConfig` DSL into an executable `SplitPlan`.
///
/// # Errors
///
/// Returns `LayoutPlanError` if the layout is structurally invalid
/// (empty columns, zero widths/heights, too many panes).
///
/// # Normalization rules
///
/// Column widths and pane heights are treated as *relative ratios*, not
/// strict percentages. They are normalized against their sibling sum.
/// This means `[33, 33, 33]` and `[1, 1, 1]` produce the same layout.
/// The last sibling absorbs any rounding remainder.
pub fn plan_layout(layout: &LayoutConfig) -> Result<SplitPlan, LayoutPlanError> {
    validate_layout(layout)?;

    let columns = &layout.columns;

    let mut steps: Vec<SplitStep> = Vec::new();
    let mut launches: Vec<PaneLaunch> = Vec::new();
    let mut total_panes: u32 = 0;

    // Phase 1: Column splits (horizontal, right-to-left).
    // The first column (index 0) occupies the initial pane — no split needed.
    // We process columns from the last to the second, splitting each off.
    let col_widths: Vec<u32> = columns.iter().map(|c| u32::from(c.width)).collect();
    let col_local_percents = compute_local_percents(&col_widths);

    for i in (1..columns.len()).rev() {
        steps.push(SplitStep {
            direction: SplitDirection::Horizontal,
            local_percent: col_local_percents[i],
            col_index: i,
            pane_index: 0,
        });
    }

    // Phase 2: Row splits within each column (vertical, bottom-to-top).
    for (ci, col) in columns.iter().enumerate() {
        let pane_heights: Vec<u32> = col.panes.iter().map(|p| u32::from(p.height)).collect();
        let pane_local_percents = compute_local_percents(&pane_heights);

        for pi in (1..col.panes.len()).rev() {
            steps.push(SplitStep {
                direction: SplitDirection::Vertical,
                local_percent: pane_local_percents[pi],
                col_index: ci,
                pane_index: pi,
            });
        }

        for (pi, pane) in col.panes.iter().enumerate() {
            launches.push(PaneLaunch {
                col_index: ci,
                pane_index: pi,
                launch: pane.launch.clone(),
            });
            total_panes += 1;
        }
    }

    Ok(SplitPlan {
        steps,
        launches,
        total_panes,
    })
}

/// Validate layout structure before planning.
///
/// A `LayoutConfig` that reaches the planner has been explicitly configured.
/// Empty `columns` is rejected — callers that have no layout should skip
/// planning entirely rather than passing an empty config.
fn validate_layout(layout: &LayoutConfig) -> Result<(), LayoutPlanError> {
    let columns = &layout.columns;

    if columns.is_empty() {
        return Err(LayoutPlanError::EmptyColumns);
    }

    let total_panes: usize = columns.iter().map(|c| c.panes.len()).sum();
    if total_panes > MAX_TOTAL_PANES {
        return Err(LayoutPlanError::TooManyPanes {
            count: total_panes,
            max: MAX_TOTAL_PANES,
        });
    }

    for (ci, col) in columns.iter().enumerate() {
        if col.width == 0 {
            return Err(LayoutPlanError::ZeroWidth { col_index: ci });
        }
        if col.panes.is_empty() {
            return Err(LayoutPlanError::EmptyPanes { col_index: ci });
        }
        for (pi, pane) in col.panes.iter().enumerate() {
            if pane.height == 0 {
                return Err(LayoutPlanError::ZeroHeight {
                    col_index: ci,
                    pane_index: pi,
                });
            }
        }
    }

    Ok(())
}

/// Compute local split percentages from sibling ratios.
///
/// Given ratios `[r0, r1, ..., rN-1]`, returns the local split percentage
/// for each sibling. Index 0 always returns 0 (no split — it's the remainder).
///
/// Splits are applied from the last sibling backwards. For sibling at index `i`:
///
/// ```text
/// local_pct[i] = round(r[i] / sum(r[0..=i]) * 100)
/// ```
///
/// This ensures that when splitting from right-to-left (or bottom-to-top),
/// each split carves out the correct proportion of the remaining space.
fn compute_local_percents(ratios: &[u32]) -> Vec<u8> {
    let len = ratios.len();
    if len <= 1 {
        return vec![0; len];
    }

    let mut result = vec![0u8; len];

    for i in 1..len {
        let remaining_sum: u32 = ratios[..=i].iter().sum();
        if remaining_sum == 0 {
            result[i] = 0;
            continue;
        }
        // Round to nearest integer: (numerator * 100 + divisor/2) / divisor
        let local =
            (u64::from(ratios[i]) * 100 + u64::from(remaining_sum) / 2) / u64::from(remaining_sum);
        result[i] = local.clamp(1, 99) as u8;
    }

    result
}

/// Validate a `LayoutConfig` at config-load time.
///
/// This is a lightweight check suitable for `AppConfig::validate()`.
/// It does not produce a `SplitPlan`, only checks structural invariants.
pub fn validate_layout_config(layout: &LayoutConfig) -> Result<(), crate::errors::ConfigError> {
    if layout.columns.is_empty() {
        return Err(crate::errors::ConfigError::InvalidValue {
            field: "layout.columns".to_owned(),
            reason: "layout block is present but columns list is empty; either remove the \
                     [layout] section or define at least one column"
                .to_owned(),
        });
    }

    let total_panes: usize = layout.columns.iter().map(|c| c.panes.len()).sum();
    if total_panes > MAX_TOTAL_PANES {
        return Err(crate::errors::ConfigError::InvalidValue {
            field: "layout".to_owned(),
            reason: format!("total pane count ({total_panes}) exceeds maximum ({MAX_TOTAL_PANES})"),
        });
    }

    for (ci, col) in layout.columns.iter().enumerate() {
        if col.width == 0 {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: format!("layout.columns[{ci}].width"),
                reason: "column width must be greater than 0".to_owned(),
            });
        }
        if col.panes.is_empty() {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: format!("layout.columns[{ci}].panes"),
                reason: "column must have at least one pane".to_owned(),
            });
        }
        for (pi, pane) in col.panes.iter().enumerate() {
            if pane.height == 0 {
                return Err(crate::errors::ConfigError::InvalidValue {
                    field: format!("layout.columns[{ci}].panes[{pi}].height"),
                    reason: "pane height must be greater than 0".to_owned(),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PaneLaunchRef;
    use crate::ports::{ColumnConfig, PaneConfig};

    fn simple_layout(col_widths: &[u8], pane_heights_per_col: &[&[u8]]) -> LayoutConfig {
        let mut cols = Vec::new();
        for (i, &width) in col_widths.iter().enumerate() {
            let heights = pane_heights_per_col.get(i).copied().unwrap_or(&[100]);
            cols.push(ColumnConfig {
                width,
                panes: heights
                    .iter()
                    .map(|&h| PaneConfig {
                        height: h,
                        launch: PaneLaunchRef::EmptyShell,
                    })
                    .collect(),
            });
        }
        LayoutConfig {
            template: Default::default(),
            columns: cols,
        }
    }

    // ── compute_local_percents unit tests ────────────────────────────────

    #[test]
    fn local_percents_single_item() {
        assert_eq!(compute_local_percents(&[100]), vec![0]);
    }

    #[test]
    fn local_percents_two_equal() {
        // [50, 50]: index 1 → 50/100*100 = 50
        let result = compute_local_percents(&[50, 50]);
        assert_eq!(result, vec![0, 50]);
    }

    #[test]
    fn local_percents_two_unequal_70_30() {
        // [70, 30]: index 1 → 30/100*100 = 30
        let result = compute_local_percents(&[70, 30]);
        assert_eq!(result, vec![0, 30]);
    }

    #[test]
    fn local_percents_three_equal_33_33_33() {
        // [33, 33, 33]:
        // index 2: remaining=99, local=33/99*100 = 33.33 → 33
        // index 1: remaining=66, local=33/66*100 = 50
        let result = compute_local_percents(&[33, 33, 33]);
        assert_eq!(result, vec![0, 50, 33]);
    }

    #[test]
    fn local_percents_three_custom_50_25_25() {
        // [50, 25, 25]:
        // index 2: remaining=100, local=25/100*100 = 25
        // index 1: remaining=75, local=25/75*100 = 33.33 → 33
        let result = compute_local_percents(&[50, 25, 25]);
        assert_eq!(result, vec![0, 33, 25]);
    }

    #[test]
    fn local_percents_four_equal_25_25_25_25() {
        // [25, 25, 25, 25]:
        // index 3: remaining=100, local=25
        // index 2: remaining=75, local=33
        // index 1: remaining=50, local=50
        let result = compute_local_percents(&[25, 25, 25, 25]);
        assert_eq!(result, vec![0, 50, 33, 25]);
    }

    #[test]
    fn local_percents_non_divisible_ratios() {
        // [40, 30, 30]:
        // index 2: remaining=100, local=30
        // index 1: remaining=70, local=30/70*100 = 42.86 → 43
        let result = compute_local_percents(&[40, 30, 30]);
        assert_eq!(result, vec![0, 43, 30]);
    }

    #[test]
    fn local_percents_unequal_ratios() {
        // [10, 20, 70]:
        // index 2: remaining=100, local=70
        // index 1: remaining=30, local=20/30*100 = 66.67 → 67
        let result = compute_local_percents(&[10, 20, 70]);
        assert_eq!(result, vec![0, 67, 70]);
    }

    // ── plan_layout column split tests ───────────────────────────────────

    #[test]
    fn plan_two_col_50_50() {
        let layout = simple_layout(&[50, 50], &[&[100], &[100]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 2);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].direction, SplitDirection::Horizontal);
        assert_eq!(plan.steps[0].local_percent, 50);
        assert_eq!(plan.steps[0].col_index, 1);
    }

    #[test]
    fn plan_three_col_33_33_33() {
        let layout = simple_layout(&[33, 33, 33], &[&[100], &[100], &[100]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 3);
        assert_eq!(plan.steps.len(), 2);

        // Columns split right-to-left: col 2 first, then col 1
        assert_eq!(plan.steps[0].col_index, 2);
        assert_eq!(plan.steps[0].local_percent, 33);

        assert_eq!(plan.steps[1].col_index, 1);
        assert_eq!(plan.steps[1].local_percent, 50);
    }

    #[test]
    fn plan_two_col_70_30() {
        let layout = simple_layout(&[70, 30], &[&[100], &[100]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 2);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].local_percent, 30);
    }

    // ── plan_layout row split tests ──────────────────────────────────────

    #[test]
    fn plan_single_col_three_rows_33_33_33() {
        let layout = simple_layout(&[100], &[&[33, 33, 33]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 3);
        assert_eq!(plan.steps.len(), 2);

        // Rows split bottom-to-top: pane 2 first, then pane 1
        assert_eq!(plan.steps[0].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[0].pane_index, 2);
        assert_eq!(plan.steps[0].local_percent, 33);

        assert_eq!(plan.steps[1].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[1].pane_index, 1);
        assert_eq!(plan.steps[1].local_percent, 50);
    }

    #[test]
    fn plan_single_col_two_rows_70_30() {
        let layout = simple_layout(&[100], &[&[70, 30]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 2);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[0].local_percent, 30);
    }

    // ── combined column + row tests ──────────────────────────────────────

    #[test]
    fn plan_two_cols_with_rows() {
        // Col 0 (60%): 2 panes [50, 50]
        // Col 1 (40%): 3 panes [33, 33, 33]
        let layout = simple_layout(&[60, 40], &[&[50, 50], &[33, 33, 33]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 5);
        // 1 col split + 1 row split (col 0) + 2 row splits (col 1) = 4 steps
        assert_eq!(plan.steps.len(), 4);

        // Step 0: col split (col 1)
        assert_eq!(plan.steps[0].direction, SplitDirection::Horizontal);
        assert_eq!(plan.steps[0].col_index, 1);
        assert_eq!(plan.steps[0].local_percent, 40);

        // Step 1: row split in col 0 (pane 1)
        assert_eq!(plan.steps[1].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[1].col_index, 0);
        assert_eq!(plan.steps[1].pane_index, 1);
        assert_eq!(plan.steps[1].local_percent, 50);

        // Step 2: row split in col 1 (pane 2)
        assert_eq!(plan.steps[2].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[2].col_index, 1);
        assert_eq!(plan.steps[2].pane_index, 2);
        assert_eq!(plan.steps[2].local_percent, 33);

        // Step 3: row split in col 1 (pane 1)
        assert_eq!(plan.steps[3].direction, SplitDirection::Vertical);
        assert_eq!(plan.steps[3].col_index, 1);
        assert_eq!(plan.steps[3].pane_index, 1);
        assert_eq!(plan.steps[3].local_percent, 50);
    }

    #[test]
    fn plan_three_cols_mixed_rows() {
        // Col 0 (33%): 1 pane
        // Col 1 (33%): 2 panes [50, 50]
        // Col 2 (33%): 3 panes [33, 33, 33]
        let layout = simple_layout(&[33, 33, 33], &[&[100], &[50, 50], &[33, 33, 33]]);
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.total_panes, 6);
        // 2 col splits + 0 row (col 0) + 1 row (col 1) + 2 rows (col 2) = 5 steps
        assert_eq!(plan.steps.len(), 5);
    }

    // ── validation tests ─────────────────────────────────────────────────

    #[test]
    fn plan_rejects_empty_columns() {
        let layout = LayoutConfig::default();
        let err = plan_layout(&layout).unwrap_err();
        assert_eq!(err, LayoutPlanError::EmptyColumns);
    }

    #[test]
    fn plan_rejects_zero_width() {
        let layout = simple_layout(&[0, 50], &[&[100], &[100]]);
        let err = plan_layout(&layout).unwrap_err();
        assert!(matches!(err, LayoutPlanError::ZeroWidth { col_index: 0 }));
    }

    #[test]
    fn plan_rejects_zero_height() {
        let layout = simple_layout(&[100], &[&[0]]);
        let err = plan_layout(&layout).unwrap_err();
        assert!(matches!(
            err,
            LayoutPlanError::ZeroHeight {
                col_index: 0,
                pane_index: 0
            }
        ));
    }

    #[test]
    fn plan_rejects_empty_panes() {
        let layout = LayoutConfig {
            template: Default::default(),
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![],
            }],
        };
        let err = plan_layout(&layout).unwrap_err();
        assert!(matches!(err, LayoutPlanError::EmptyPanes { col_index: 0 }));
    }

    #[test]
    fn plan_rejects_too_many_panes() {
        let mut panes = Vec::new();
        for _ in 0..=MAX_TOTAL_PANES {
            panes.push(PaneConfig {
                height: 10,
                launch: PaneLaunchRef::EmptyShell,
            });
        }
        let layout = LayoutConfig {
            template: Default::default(),
            columns: vec![ColumnConfig { width: 100, panes }],
        };
        let err = plan_layout(&layout).unwrap_err();
        assert!(matches!(err, LayoutPlanError::TooManyPanes { .. }));
    }

    // ── launch association tests ─────────────────────────────────────────

    #[test]
    fn plan_preserves_launch_refs() {
        use crate::domain::{CommandSpec, ToolName};

        let lazygit = PaneLaunchRef::Tool {
            name: ToolName::new("lazygit").unwrap(),
        };
        let cargo_test = PaneLaunchRef::InlineCommand {
            spec: CommandSpec::new("cargo", ["test", "--workspace"]),
        };

        let layout = LayoutConfig {
            template: Default::default(),
            columns: vec![
                ColumnConfig {
                    width: 60,
                    panes: vec![
                        PaneConfig {
                            height: 50,
                            launch: PaneLaunchRef::EmptyShell,
                        },
                        PaneConfig {
                            height: 50,
                            launch: lazygit.clone(),
                        },
                    ],
                },
                ColumnConfig {
                    width: 40,
                    panes: vec![PaneConfig {
                        height: 100,
                        launch: cargo_test.clone(),
                    }],
                },
            ],
        };
        let plan = plan_layout(&layout).unwrap();

        assert_eq!(plan.launches.len(), 3);
        assert_eq!(plan.launches[0].launch, PaneLaunchRef::EmptyShell);
        assert_eq!(plan.launches[1].launch, lazygit);
        assert_eq!(plan.launches[2].launch, cargo_test);
    }

    // ── determinism test ─────────────────────────────────────────────────

    #[test]
    fn plan_is_deterministic() {
        let layout = simple_layout(&[33, 33, 33], &[&[33, 33, 33], &[50, 50], &[70, 30]]);
        let plan1 = plan_layout(&layout).unwrap();
        let plan2 = plan_layout(&layout).unwrap();
        assert_eq!(plan1, plan2);
    }

    // ── config validation tests ──────────────────────────────────────────

    #[test]
    fn validate_config_passes_valid_layout() {
        let layout = simple_layout(&[50, 50], &[&[50, 50], &[100]]);
        assert!(validate_layout_config(&layout).is_ok());
    }

    #[test]
    fn validate_config_rejects_zero_width() {
        let layout = simple_layout(&[0, 100], &[&[100], &[100]]);
        let err = validate_layout_config(&layout).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("width"));
    }

    #[test]
    fn validate_config_rejects_zero_height() {
        let layout = simple_layout(&[100], &[&[0]]);
        let err = validate_layout_config(&layout).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("height"));
    }

    #[test]
    fn validate_config_rejects_empty_panes() {
        let layout = LayoutConfig {
            template: Default::default(),
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![],
            }],
        };
        let err = validate_layout_config(&layout).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("at least one pane"));
    }

    #[test]
    fn validate_config_rejects_empty_columns() {
        let layout = LayoutConfig::default();
        let err = validate_layout_config(&layout).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("empty"),
            "expected empty columns rejection: {msg}"
        );
    }
}
