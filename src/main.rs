use actix_web::{dev::Service, dev::ServiceRequest, dev::ServiceResponse, Error};
use futures_util::future::LocalBoxFuture;
use actix_web::{web, App, HttpServer, Responder, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::fs;
use std::io::{self, Write};
use chrono::{NaiveDateTime, Utc};
use actix_cors::Cors;
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

const VALID_COURSE_CODES: &[&str] = &["OTHER", "COMP7082", "COMP7035", "COMP7071", "COMP7003", "MATH7808"];
const DATA_FILE: &str = "/data/homework.json";

fn load_assignments() -> (Vec<Homework>, u64) {
    if let Ok(data) = fs::read_to_string(DATA_FILE) {
        if let Ok(assignments) = serde_json::from_str::<Vec<Homework>>(&data) {
            let max_id = assignments.iter().filter_map(|hw| hw.id).max().unwrap_or(0);
            return (assignments, max_id + 1);
        }
    }
    (Vec::new(), 1)
}

fn save_assignments(assignments: &Vec<Homework>) {
    if let Ok(data) = serde_json::to_string(assignments) {
        let _ = fs::write(DATA_FILE, data);
    }
}

struct ExitMiddleware;

impl<S, B> actix_web::dev::Transform<S, ServiceRequest> for ExitMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = ExitMiddlewareService<S>;
    type InitError = ();
    type Future = LocalBoxFuture<'static, Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        Box::pin(async move { Ok(ExitMiddlewareService { service }) })
    }
}

struct ExitMiddlewareService<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for ExitMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let fut = self.service.call(req);

        Box::pin(async move {
            let res = fut.await?;
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                std::process::exit(0);
            });
            Ok(res)
        })
    }
}

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
            save_assignments(&assignments);
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
        save_assignments(&assignments);
        return HttpResponse::Ok().json(ApiResponse { data: "Created" });
    }
}

async fn delete_homework(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let id = match query.get("id").and_then(|id_str| id_str.parse::<u64>().ok()) {
        Some(id) => id,
        None => return HttpResponse::BadRequest().body("Invalid or missing id parameter"),
    };

    let mut assignments = state.assignments.lock().unwrap();
    if let Some(pos) = assignments.iter().position(|h| h.id == Some(id)) {
        assignments.remove(pos);
        save_assignments(&assignments);
        HttpResponse::Ok().json(ApiResponse { data: "Deleted" })
    } else {
        HttpResponse::NotFound().body("Homework not found")
    }
}

async fn get_assignments(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let mut assignments = state.assignments.lock().unwrap();
    let now = Utc::now().timestamp();

    assignments.retain(|assignment| assignment.due_date >= now);

    if let Some(format) = query.get("format") {
        if format == "html" {
            let mut html = String::from("<html><body><ul>");
            for assignment in &*assignments {
                let due_date = chrono::NaiveDateTime::from_timestamp(assignment.due_date, 0)
                    .and_local_timezone(chrono::Utc)
                    .single()
                    .unwrap_or_else(|| chrono::Utc::now());
                html.push_str(&format!(
                    "<li>ID: {} | Name: {} | Course Code: {} | Due Date: {}</li>",
                    assignment.id.unwrap_or(0),
                    assignment.name,
                    assignment.course_code,
                    due_date.to_string(),
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let (assignments, next_id) = load_assignments();
    let state = web::Data::new(AppState {
        assignments: Mutex::new(assignments),
        next_id: Mutex::new(next_id),
    });

    HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()//allow all origins
                    .allowed_methods(vec!["GET","POST","DELETE"])
                    .max_age(72000),
            )
            .app_data(state.clone())
            .wrap(ExitMiddleware)
            .route("/homework", web::post().to(create_or_update_homework))
            .route("/homework", web::get().to(get_assignments))
            .route("/homework", web::delete().to(delete_homework))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

