//! What "matches" means for a search. One definition shared by every searcher, so a query behaves
//! the same on a folder on disk, a cloud folder or an in-memory folder. Which file names carry an
//! extension worth dropping is the app's to say (the file types it opens), so the caller passes
//! that test in.

/// True when every space separated term of `query` appears in `name`, in any order and any case.
/// An empty query matches everything (callers decide whether to search at all). Underscores read
/// as spaces on both sides, so "gear box" finds "gear_box" and a query pasted from a file name
/// finds that file. With `content_file`, a name it calls a content file (`gear box.stl` when the
/// app opens `.stl`) has its extension dropped first, so "stl" does not match every mesh; `None`
/// matches the name whole, which is what an item that is not a file needs. No query (`None`) is
/// not a match: a search that has nothing to look for finds nothing.
pub fn name_matches<'q>(
    name: &str,
    query: impl Into<Option<&'q str>>,
    content_file: Option<&dyn Fn(&str) -> bool>,
) -> bool {
    let Some(query) = query.into() else {
        return false;
    };
    let stripped = match content_file {
        Some(is_content_file) => without_content_extension(name, is_content_file),
        None => name,
    };
    let name_with_spaces = stripped.replace('_', " ").to_lowercase();
    query
        .replace('_', " ")
        .split(' ')
        .filter(|term| !term.is_empty())
        .all(|term| name_with_spaces.contains(&term.to_lowercase()))
}

/// Drops the extension only when `is_content_file` says the name is a file the app opens. A name
/// is not a path: "Rev 1.2 bracket" has no extension, and cutting at its dot would make most of
/// the name unsearchable.
fn without_content_extension<'n>(name: &'n str, is_content_file: &dyn Fn(&str) -> bool) -> &'n str {
    if is_content_file(name) {
        // The name up to its last dot.
        match name.rfind('.') {
            Some(dot) => &name[..dot],
            None => name,
        }
    } else {
        name
    }
}
