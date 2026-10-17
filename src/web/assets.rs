use super::Reply;
macro_rules! asset {
    ($mime:expr,$path:literal) => {
        Some(Reply::asset(
            $mime,
            include_str!(concat!("../../web/", $path)),
        ))
    };
}
pub(super) fn get(route: &str) -> Option<Reply> {
    match route {
        "/" => asset!("text/html; charset=utf-8", "index.html"),
        "/styles/base.css" => asset!("text/css", "styles/base.css"),
        "/styles/layout.css" => asset!("text/css", "styles/layout.css"),
        "/styles/components.css" => asset!("text/css", "styles/components.css"),
        "/styles/tree.css" => asset!("text/css", "styles/tree.css"),
        "/styles/auth.css" => asset!("text/css", "styles/auth.css"),
        "/js/app.js" => asset!("text/javascript", "js/app.js"),
        "/js/api.js" => asset!("text/javascript", "js/api.js"),
        "/js/dom.js" => asset!("text/javascript", "js/dom.js"),
        "/js/components.js" => asset!("text/javascript", "js/components.js"),
        "/js/overview.js" => asset!("text/javascript", "js/overview.js"),
        "/js/tree.js" => asset!("text/javascript", "js/tree.js"),
        "/js/geometry.js" => asset!("text/javascript", "js/geometry.js"),
        "/js/details.js" => asset!("text/javascript", "js/details.js"),
        "/js/chart.js" => asset!("text/javascript", "js/chart.js"),
        "/js/explorer.js" => asset!("text/javascript", "js/explorer.js"),
        "/js/history.js" => asset!("text/javascript", "js/history.js"),
        "/js/delete.js" => asset!("text/javascript", "js/delete.js"),
        "/js/login.js" => asset!("text/javascript", "js/login.js"),
        "/js/cards.js" => asset!("text/javascript", "js/cards.js"),
        "/js/cleanup.js" => asset!("text/javascript", "js/cleanup.js"),
        _ => None,
    }
}
