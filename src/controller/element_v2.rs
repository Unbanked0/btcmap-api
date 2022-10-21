use crate::db;
use crate::model::ApiError;
use crate::model::Element;
use actix_web::get;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::web::Path;
use actix_web::web::Query;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;

#[derive(Deserialize)]
pub struct GetArgs {
    updated_since: Option<String>,
}

#[derive(Serialize)]
pub struct GetItem {
    pub id: String,
    pub osm_json: Value,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: String,
}

impl Into<GetItem> for Element {
    fn into(self) -> GetItem {
        GetItem {
            id: self.id,
            osm_json: self.data,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at.unwrap_or("".into()),
        }
    }
}

#[get("/v2/elements")]
pub async fn get(
    args: Query<GetArgs>,
    conn: Data<Mutex<Connection>>,
) -> Result<Json<Vec<GetItem>>, ApiError> {
    Ok(Json(match &args.updated_since {
        Some(updated_since) => conn
            .lock()?
            .prepare(db::ELEMENT_SELECT_UPDATED_SINCE)?
            .query_map([updated_since], db::mapper_element_full())?
            .filter(|it| it.is_ok())
            .map(|it| it.unwrap().into())
            .collect(),
        None => conn
            .lock()?
            .prepare(db::ELEMENT_SELECT_ALL)?
            .query_map([], db::mapper_element_full())?
            .filter(|it| it.is_ok())
            .map(|it| it.unwrap().into())
            .collect(),
    }))
}

#[get("/v2/elements/{id}")]
pub async fn get_by_id(
    path: Path<String>,
    conn: Data<Mutex<Connection>>,
) -> Result<Json<Option<GetItem>>, ApiError> {
    Ok(Json(
        conn.lock()?
            .query_row(
                db::ELEMENT_SELECT_BY_ID,
                [path.into_inner()],
                db::mapper_element_full(),
            )
            .optional()?
            .map(|it| it.into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use actix_web::test::TestRequest;
    use actix_web::{test, App};
    use rusqlite::named_params;
    use std::sync::atomic::Ordering;

    #[actix_web::test]
    async fn get_v2_empty_table() {
        let db_name = db::COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut db =
            Connection::open(format!("file::testdb_{db_name}:?mode=memory&cache=shared")).unwrap();
        db::migrate(&mut db).unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::new(Mutex::new(db)))
                .service(super::get),
        )
        .await;
        let req = TestRequest::get().uri("/v2/elements").to_request();
        let res: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.as_array().unwrap().len(), 0);
    }

    #[actix_web::test]
    async fn get_v2_one_row() {
        let db_name = db::COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut db =
            Connection::open(format!("file::testdb_{db_name}:?mode=memory&cache=shared")).unwrap();
        db::migrate(&mut db).unwrap();
        db.execute(
            db::ELEMENT_INSERT,
            named_params! {
                ":id": "node:1",
                ":data": "{}",
            },
        )
        .unwrap();
        let app = test::init_service(
            App::new()
                .app_data(Data::new(Mutex::new(db)))
                .service(super::get),
        )
        .await;
        let req = TestRequest::get().uri("/v2/elements").to_request();
        let res: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.as_array().unwrap().len(), 1);
    }
}