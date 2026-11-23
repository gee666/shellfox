use super::*;

fn row(id: u64) -> SemanticNode {
    SemanticNode::new(SemanticId(id), Some(SemanticId(1)), Role::Button, "Row")
}

/// One frame: the root, then a cached pane that either renders `fresh` nodes or is reused.
fn paint(frame: &SemanticFrame, cache: &SemanticCache, fresh: Option<&[u64]>) -> Vec<SemanticId> {
    let generation = frame.begin_frame();
    frame.record(SemanticNode::new(SemanticId(1), None, Role::Window, "Shellfox"), false);
    let start = frame.recorded();
    if let Some(rows) = fresh {
        cache.rendered();
        for id in rows {
            frame.record(row(*id), false);
        }
    }
    frame.settle_cached(cache, start, |_| false);
    let tree = frame.finish_frame(generation).unwrap();
    [2, 3].into_iter().map(SemanticId).filter(|id| tree.node(*id).is_some()).collect()
}

#[test]
fn a_reused_cached_pane_republishes_the_nodes_it_last_painted() {
    let frame = SemanticFrame::new(SemanticId(1));
    let cache = SemanticCache::default();
    assert_eq!(paint(&frame, &cache, Some(&[2, 3])), [SemanticId(2), SemanticId(3)]);
    assert_eq!(paint(&frame, &cache, None), [SemanticId(2), SemanticId(3)]);
    assert_eq!(paint(&frame, &cache, Some(&[])), []);
    assert_eq!(paint(&frame, &cache, None), []);
}
