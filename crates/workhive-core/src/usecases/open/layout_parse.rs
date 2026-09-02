#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};

#[allow(unused_imports)]
use camino::Utf8PathBuf;

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    CommandSpec, MaterializationKind, PaneDirection, PaneLaunchIntent, PaneLaunchRef,
    ReattachOutcome, RuntimeBackendId, SessionAvailability, ToolCatalog, ToolName, WorkspaceId,
    WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider, classify_reattach_outcome,
};
#[allow(unused_imports)]
use crate::errors::SnapshotError;
#[allow(unused_imports)]
use crate::ports::{
    CreateWorkspaceRuntimeRequest, ExecutableAvailabilityPort, PaneRuntimePort, ProvisionError,
    ProvisionOutcome, ProvisionWorkspaceRequest, RuntimeError, SendInputRequest,
    SnapshotPaneRuntimePort, SnapshotRuntimePort, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// A parsed tmux layout tree node.
///
/// tmux encodes layouts as a recursive structure:
///   - Leaf: `<width>x<height>,<x>,<y>,<pane_id>`
///   - Horizontal split (children left-to-right): `<w>x<h>,<x>,<y>{child,child,...}`
///   - Vertical split (children top-to-bottom): `<w>x<h>,<x>,<y>[child,child,...]`
///
/// The top-level string starts with a checksum followed by `,` then the root node.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LayoutNode {
    Leaf { pane_id: u32 },
    HSplit { children: Vec<LayoutNode> },
    VSplit { children: Vec<LayoutNode> },
}

/// A split instruction derived from the parsed layout tree.
///
/// Each entry describes: which existing pane to split, in which direction,
/// to produce the pane that will be placed at a given leaf position.
#[derive(Debug, Clone)]
pub(crate) struct SplitInstruction {
    pub(crate) split_target_leaf_index: usize,
    pub(crate) direction: PaneDirection,
}

impl LayoutNode {
    #[cfg(test)]
    fn leaves(&self) -> Vec<u32> {
        match self {
            LayoutNode::Leaf { pane_id } => vec![*pane_id],
            LayoutNode::HSplit { children } | LayoutNode::VSplit { children } => {
                children.iter().flat_map(|c| c.leaves()).collect()
            }
        }
    }

    pub(crate) fn leaf_count(&self) -> usize {
        match self {
            LayoutNode::Leaf { .. } => 1,
            LayoutNode::HSplit { children } | LayoutNode::VSplit { children } => {
                children.iter().map(|c| c.leaf_count()).sum()
            }
        }
    }
}

/// Parse a tmux layout string into a `LayoutNode` tree.
///
/// Returns `None` if the layout string cannot be parsed.
pub(crate) fn parse_layout(layout: &str) -> Option<LayoutNode> {
    let body = if let Some((_checksum, rest)) = layout.split_once(',') {
        rest
    } else {
        return None;
    };
    let (node, _) = parse_node(body.as_bytes(), 0)?;
    Some(node)
}

pub(crate) fn parse_node(bytes: &[u8], start: usize) -> Option<(LayoutNode, usize)> {
    let mut pos = start;

    // Skip `<width>x<height>,<x>,<y>` geometry prefix.
    // Width
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'x' {
        return None;
    }
    pos += 1; // skip 'x'
    // Height
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b',' {
        return None;
    }
    pos += 1; // skip ','
    // X offset
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b',' {
        return None;
    }
    pos += 1; // skip ','
    // Y offset
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    if pos >= bytes.len() {
        return None;
    }

    match bytes[pos] {
        b'{' | b'[' => {
            let is_hsplit = bytes[pos] == b'{';
            let close = if is_hsplit { b'}' } else { b']' };
            pos += 1; // skip opening bracket
            let mut children = Vec::new();
            loop {
                if pos >= bytes.len() {
                    return None;
                }
                if bytes[pos] == close {
                    pos += 1;
                    break;
                }
                if bytes[pos] == b',' && !children.is_empty() {
                    pos += 1; // skip separator between children
                }
                let (child, next_pos) = parse_node(bytes, pos)?;
                children.push(child);
                pos = next_pos;
            }
            let node = if is_hsplit {
                LayoutNode::HSplit { children }
            } else {
                LayoutNode::VSplit { children }
            };
            Some((node, pos))
        }
        b',' => {
            pos += 1; // skip ','
            let id_start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            let pane_id: u32 = std::str::from_utf8(&bytes[id_start..pos])
                .ok()?
                .parse()
                .ok()?;
            Some((LayoutNode::Leaf { pane_id }, pos))
        }
        _ => None,
    }
}

/// Derive a sequence of split instructions from a parsed layout tree.
///
/// The strategy performs a depth-first traversal. The first leaf of
/// the tree corresponds to the initial pane (created via `create_window`).
/// Each subsequent leaf requires a split from its **sibling predecessor**
/// within the same parent container. The direction is determined by the
/// parent container type (HSplit -> Right, VSplit -> Down).
pub(crate) fn derive_split_instructions(tree: &LayoutNode) -> Vec<SplitInstruction> {
    let mut instructions = Vec::new();
    let mut leaf_counter = 0usize;
    walk_for_splits(tree, None, &mut leaf_counter, &mut instructions);
    instructions
}

pub(crate) fn walk_for_splits(
    node: &LayoutNode,
    inherited_direction: Option<PaneDirection>,
    leaf_counter: &mut usize,
    instructions: &mut Vec<SplitInstruction>,
) {
    match node {
        LayoutNode::Leaf { .. } => {
            if *leaf_counter == 0 {
                *leaf_counter += 1;
                return;
            }
            let direction = inherited_direction.unwrap_or(PaneDirection::Down);
            let target_leaf = leaf_counter.saturating_sub(1);
            instructions.push(SplitInstruction {
                split_target_leaf_index: target_leaf,
                direction,
            });
            *leaf_counter += 1;
        }
        LayoutNode::HSplit { children } => {
            for (i, child) in children.iter().enumerate() {
                let dir = if i == 0 {
                    inherited_direction
                } else {
                    Some(PaneDirection::Right)
                };
                walk_for_splits(child, dir, leaf_counter, instructions);
            }
        }
        LayoutNode::VSplit { children } => {
            for (i, child) in children.iter().enumerate() {
                let dir = if i == 0 {
                    inherited_direction
                } else {
                    Some(PaneDirection::Down)
                };
                walk_for_splits(child, dir, leaf_counter, instructions);
            }
        }
    }
}

/// Fallback for unparseable layouts: creates N splits all using `Down`
/// direction, each targeting the most recently created pane.
pub(crate) fn fallback_split_instructions(split_count: usize) -> Vec<SplitInstruction> {
    (0..split_count)
        .map(|i| SplitInstruction {
            split_target_leaf_index: i,
            direction: PaneDirection::Down,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use crate::ports::{AttachWorkspaceRequest, SendInputRequest, WorkspaceRuntimeInfo};

    #[test]
    fn parse_layout_simple_hsplit_two_panes() {
        let layout = "ab12,240x67,0,0{120x67,0,0,0,119x67,121,0,1}";
        let tree = parse_layout(layout).expect("should parse");
        assert_eq!(tree.leaf_count(), 2);
        assert_eq!(tree.leaves(), vec![0, 1]);
        match &tree {
            LayoutNode::HSplit { children } => assert_eq!(children.len(), 2),
            other => panic!("expected HSplit, got {other:?}"),
        }
    }

    #[test]
    fn parse_layout_nested_hsplit_vsplit() {
        let layout = "6f10,240x67,0,0{120x67,0,0,0,119x67,121,0[119x33,121,0,2,119x33,121,34,3]}";
        let tree = parse_layout(layout).expect("should parse");
        assert_eq!(tree.leaf_count(), 3);
        assert_eq!(tree.leaves(), vec![0, 2, 3]);
        match &tree {
            LayoutNode::HSplit { children } => {
                assert_eq!(children.len(), 2);
                assert!(matches!(children[0], LayoutNode::Leaf { pane_id: 0 }));
                match &children[1] {
                    LayoutNode::VSplit { children: inner } => {
                        assert_eq!(inner.len(), 2);
                    }
                    other => panic!("expected VSplit, got {other:?}"),
                }
            }
            other => panic!("expected HSplit, got {other:?}"),
        }
    }

    #[test]
    fn parse_layout_single_pane() {
        let layout = "1234,80x24,0,0,0";
        let tree = parse_layout(layout).expect("should parse");
        assert_eq!(tree.leaf_count(), 1);
        assert!(matches!(tree, LayoutNode::Leaf { pane_id: 0 }));
    }

    #[test]
    fn parse_layout_vsplit_three_panes() {
        let layout = "cafe,80x60,0,0[80x19,0,0,0,80x19,0,20,1,80x19,0,40,2]";
        let tree = parse_layout(layout).expect("should parse");
        assert_eq!(tree.leaf_count(), 3);
        assert_eq!(tree.leaves(), vec![0, 1, 2]);
        match &tree {
            LayoutNode::VSplit { children } => assert_eq!(children.len(), 3),
            other => panic!("expected VSplit, got {other:?}"),
        }
    }

    #[test]
    fn parse_layout_deeply_nested() {
        // {A, [B, {C, D}]}
        let layout = "dead,100x50,0,0{50x50,0,0,0,49x50,51,0[49x24,51,0,1,49x24,51,25{24x24,51,25,2,24x24,76,25,3}]}";
        let tree = parse_layout(layout).expect("should parse");
        assert_eq!(tree.leaf_count(), 4);
        assert_eq!(tree.leaves(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn derive_split_instructions_hsplit_two_panes() {
        let layout = "ab12,240x67,0,0{120x67,0,0,0,119x67,121,0,1}";
        let tree = parse_layout(layout).unwrap();
        let instructions = derive_split_instructions(&tree);
        assert_eq!(instructions.len(), 1);
        assert_eq!(instructions[0].split_target_leaf_index, 0);
        assert_eq!(instructions[0].direction, PaneDirection::Right);
    }

    #[test]
    fn derive_split_instructions_nested_layout() {
        let layout = "6f10,240x67,0,0{120x67,0,0,0,119x67,121,0[119x33,121,0,2,119x33,121,34,3]}";
        let tree = parse_layout(layout).unwrap();
        let instructions = derive_split_instructions(&tree);
        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0].direction, PaneDirection::Right);
        assert_eq!(instructions[0].split_target_leaf_index, 0);
        assert_eq!(instructions[1].direction, PaneDirection::Down);
        assert_eq!(instructions[1].split_target_leaf_index, 1);
    }

    #[test]
    fn derive_split_instructions_deeply_nested() {
        // {A, [B, {C, D}]}  → 4 leaves, 3 splits
        let layout = "dead,100x50,0,0{50x50,0,0,0,49x50,51,0[49x24,51,0,1,49x24,51,25{24x24,51,25,2,24x24,76,25,3}]}";
        let tree = parse_layout(layout).unwrap();
        let instructions = derive_split_instructions(&tree);
        assert_eq!(instructions.len(), 3);
        assert_eq!(instructions[0].direction, PaneDirection::Right);
        assert_eq!(instructions[0].split_target_leaf_index, 0);
        assert_eq!(instructions[1].direction, PaneDirection::Down);
        assert_eq!(instructions[1].split_target_leaf_index, 1);
        assert_eq!(instructions[2].direction, PaneDirection::Right);
        assert_eq!(instructions[2].split_target_leaf_index, 2);
    }

    #[test]
    fn derive_split_instructions_vsplit_three_panes() {
        let layout = "cafe,80x60,0,0[80x19,0,0,0,80x19,0,20,1,80x19,0,40,2]";
        let tree = parse_layout(layout).unwrap();
        let instructions = derive_split_instructions(&tree);
        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0].direction, PaneDirection::Down);
        assert_eq!(instructions[0].split_target_leaf_index, 0);
        assert_eq!(instructions[1].direction, PaneDirection::Down);
        assert_eq!(instructions[1].split_target_leaf_index, 1);
    }

    #[test]
    fn fallback_split_instructions_creates_sequential_down_splits() {
        let instructions = fallback_split_instructions(3);
        assert_eq!(instructions.len(), 3);
        for (i, inst) in instructions.iter().enumerate() {
            assert_eq!(inst.split_target_leaf_index, i);
            assert_eq!(inst.direction, PaneDirection::Down);
        }
    }
}
