use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use chrono::{NaiveDateTime, Utc};

#[derive(Serialize, Deserialize, Clone)]
struct Homework {
    id: Option<u64>,
    name: String,
    course_code: String,
    due_date: i64, // Unix timestamp
}

#[derive(Serialize)]
struct ApiResponse<T> {
    data: T,
}

struct AppState {
    assignments: Mutex<Vec<Homework>>,
    next_id: Mutex<u64>,
}

const VALID_COURSE_CODES: &[&str] = &["COMP7082", "COMP7035", "COMP7071", "MATH7808", "COMP7003"];

async fn create_or_update_homework(
    state: web::Data<AppState>,
    homework: web::Json<Homework>,
) -> impl Responder {
    let mut assignments = state.assignments.lock().unwrap();
    let mut next_id = state.next_id.lock().unwrap();

    if !VALID_COURSE_CODES.contains(&homework.course_code.as_str()) {
        return HttpResponse::BadRequest().body("Invalid course code");
    }

    if let Some(id) = homework.id {
        if let Some(existing) = assignments.iter_mut().find(|h| h.id == Some(id)) {
            *existing = homework.into_inner();
            return HttpResponse::Ok().json(ApiResponse { data: "Updated" });
        } else {
            return HttpResponse::NotFound().body("Homework not found");
        }
    } else {
        let new_id = *next_id;
        *next_id += 1;
        let mut new_homework = homework.into_inner();
        new_homework.id = Some(new_id);
        assignments.push(new_homework);
        return HttpResponse::Ok().json(ApiResponse { data: "Created" });
    }
}

async fn get_assignments(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let assignments = state.assignments.lock().unwrap();

    if let Some(format) = query.get("format") {
        if format == "html" {
            let mut html = String::from("<html><body><ul>");
            for assignment in &*assignments {
                html.push_str(&format!(
                    "<li>ID: {} | Name: {} | Course Code: {} | Due Date: {}</li>",
                    assignment.id.unwrap_or(0),
                    assignment.name,
                    assignment.course_code,
                    NaiveDateTime::from_timestamp(assignment.due_date, 0).to_string()
                ));
            }
            html.push_str("</ul></body></html>");
            return HttpResponse::Ok().body(html);
        }
    }

    HttpResponse::Ok().json(ApiResponse {
        data: assignments.clone(),
    })
}

async fn api_docs() -> impl Responder {
    let docs = r#"
    API Documentation:

    POST /homework - Create or update a homework assignment
      - Body (JSON):
        {
          "id": <u64>, // Optional, leave empty for new assignments
          "name": <String>,
          "course_code": <String>,
          "due_date": <i64> // Unix timestamp
        }

    GET /homework - Get all homework assignments
      - Query Parameters:
        format=html (Optional, defaults to JSON response)

    GET /docs - Get API documentation
    "#;
    HttpResponse::Ok().body(docs)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let state = web::Data::new(AppState {
        assignments: Mutex::new(Vec::new()),
        next_id: Mutex::new(1),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/homework", web::post().to(create_or_update_homework))
            .route("/homework", web::get().to(get_assignments))
            .route("/docs", web::get().to(api_docs))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}

