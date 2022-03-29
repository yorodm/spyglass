use rocket::response::status::BadRequest;
use rocket::serde::json::Json;
use rocket::State;
use sea_orm::prelude::*;
use sea_orm::Set;
use serde::Deserialize;
use url::Url;

use shared::response::{AppStatus, SearchMeta, SearchResult, SearchResults};

use super::response;
use crate::models::crawl_queue;
use crate::search::Searcher;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchReq<'r> {
    pub term: &'r str,
}

#[post("/search", data = "<search_req>")]
pub async fn search(
    state: &State<AppState>,
    search_req: Json<SearchReq<'_>>,
) -> Result<Json<SearchResults>, BadRequest<String>> {
    let fields = Searcher::doc_fields();

    let index = state.index.lock().unwrap();
    let searcher = index.reader.searcher();

    let docs = Searcher::search_with_lens(
        &state.config.lenses,
        &index.index,
        &index.reader,
        search_req.term,
    );

    let mut results: Vec<SearchResult> = Vec::new();
    for (_score, doc_addr) in docs {
        let retrieved = searcher.doc(doc_addr).unwrap();

        let title = retrieved.get_first(fields.title).unwrap();
        let description = retrieved.get_first(fields.description).unwrap();
        let url = retrieved.get_first(fields.url).unwrap();

        let result = SearchResult {
            title: title.as_text().unwrap().to_string(),
            description: description.as_text().unwrap().to_string(),
            url: url.as_text().unwrap().to_string(),
        };

        results.push(result);
    }

    let meta = SearchMeta {
        query: search_req.term.to_string(),
        num_docs: searcher.num_docs(),
        wall_time_ms: 1000,
    };

    Ok(Json(SearchResults { results, meta }))
}

/// Show the list of URLs in the queue and their status
#[get("/queue")]
pub async fn list_queue(
    state: &State<AppState>,
) -> Result<Json<response::ListQueue>, BadRequest<String>> {
    let db = &state.db;
    let queue = crawl_queue::Entity::find().all(db).await;

    match queue {
        Ok(queue) => Ok(Json(response::ListQueue { queue })),
        Err(err) => Err(BadRequest(Some(err.to_string()))),
    }
}

#[derive(Debug, Deserialize)]
pub struct QueueItem<'r> {
    pub url: &'r str,
    pub force_crawl: bool,
}

/// Add url to queue
#[post("/queue", data = "<queue_item>")]
pub async fn add_queue(
    state: &State<AppState>,
    queue_item: Json<QueueItem<'_>>,
) -> Result<&'static str, BadRequest<String>> {
    let db = &state.db;

    let parsed = Url::parse(queue_item.url).unwrap();
    let new_task = crawl_queue::ActiveModel {
        domain: Set(parsed.host_str().unwrap().to_string()),
        url: Set(queue_item.url.to_owned()),
        force_crawl: Set(queue_item.force_crawl),
        ..Default::default()
    };

    match new_task.insert(db).await {
        Ok(_) => Ok("ok"),
        Err(err) => Err(BadRequest(Some(err.to_string()))),
    }
}

pub fn _get_current_status(state: &State<AppState>) -> AppStatus {
    // Grab crawler status
    let app_state = &state.app_state;
    let paused_status = app_state.get("paused").unwrap();
    let is_paused = *paused_status == *"true";

    // Grab details about index
    let index = state.index.lock().unwrap();
    let reader = index.reader.searcher();

    AppStatus {
        num_docs: reader.num_docs(),
        is_paused,
    }
}

/// Fun stats about index size, etc.
#[get("/status")]
pub fn app_stats(state: &State<AppState>) -> Json<AppStatus> {
    Json(_get_current_status(state))
}

#[derive(Debug, Deserialize)]
pub struct UpdateStatusRequest {
    pub toggle_pause: Option<bool>,
}

#[post("/status", data = "<update_status>")]
pub async fn update_app_status(
    state: &State<AppState>,
    update_status: Json<UpdateStatusRequest>,
) -> Result<Json<AppStatus>, BadRequest<String>> {
    // Update status
    if update_status.toggle_pause.is_some() {
        let app_state = &state.app_state;
        let mut paused_status = app_state.get_mut("paused").unwrap();

        let current_status = paused_status.to_string() == "true";
        let updated_status = !current_status;
        *paused_status = updated_status.to_string();
    }

    Ok(Json(_get_current_status(state)))
}
