use std::collections::HashMap;

use crate::model::OsmUserJson;
use crate::model::User;
use crate::service::auth::get_admin_token;
use crate::ApiError;
use actix_web::get;
use actix_web::patch;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::web::Path;
use actix_web::web::Query;
use actix_web::HttpRequest;
use actix_web::HttpResponse;
use actix_web::Responder;
use rusqlite::Connection;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::warn;

#[derive(Deserialize)]
pub struct GetArgs {
    updated_since: Option<String>,
    limit: Option<i32>,
}

#[derive(Serialize, Deserialize)]
pub struct GetItem {
    pub id: i32,
    pub osm_json: OsmUserJson,
    pub tags: HashMap<String, Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub deleted_at: String,
}

impl Into<GetItem> for User {
    fn into(self) -> GetItem {
        GetItem {
            id: self.id,
            osm_json: self.osm_json,
            tags: self.tags,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self
                .deleted_at
                .map(|it| it.format(&Rfc3339).unwrap())
                .unwrap_or_default()
                .into(),
        }
    }
}

impl Into<Json<GetItem>> for User {
    fn into(self) -> Json<GetItem> {
        Json(self.into())
    }
}

#[get("")]
async fn get(args: Query<GetArgs>, conn: Data<Connection>) -> Result<Json<Vec<GetItem>>, ApiError> {
    Ok(Json(match &args.updated_since {
        Some(updated_since) => User::select_updated_since(updated_since, args.limit, &conn)?
            .into_iter()
            .map(|it| it.into())
            .collect(),
        None => User::select_all(args.limit, &conn)?
            .into_iter()
            .map(|it| it.into())
            .collect(),
    }))
}

#[get("{id}")]
pub async fn get_by_id(id: Path<i32>, conn: Data<Connection>) -> Result<Json<GetItem>, ApiError> {
    let id = id.into_inner();

    User::select_by_id(id, &conn)?
        .map(|it| it.into())
        .ok_or(ApiError::new(
            404,
            &format!("User with id = {id} doesn't exist"),
        ))
}

#[patch("{id}/tags")]
async fn patch_tags(
    args: Json<HashMap<String, Value>>,
    conn: Data<Connection>,
    id: Path<i32>,
    req: HttpRequest,
) -> Result<impl Responder, ApiError> {
    let token = get_admin_token(&conn, &req)?;
    let user_id = id.into_inner();

    let keys: Vec<String> = args.keys().map(|it| it.to_string()).collect();

    warn!(
        actor_id = token.user_id,
        user_id,
        tags = keys.join(", "),
        "User attempted to update user tags",
    );

    User::select_by_id(user_id, &conn)?.ok_or(ApiError::new(
        404,
        &format!("User with id = {user_id} doesn't exist"),
    ))?;

    User::merge_tags(user_id, &args, &conn)?;

    Ok(HttpResponse::Ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::db;
    use crate::model::token;
    use crate::Result;
    use actix_web::test::TestRequest;
    use actix_web::web::scope;
    use actix_web::{test, App};
    use reqwest::StatusCode;
    use rusqlite::named_params;
    use serde_json::{json, Value};

    #[actix_web::test]
    async fn get_empty_table() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        db::migrate(&mut conn)?;

        let app = test::init_service(
            App::new()
                .app_data(Data::new(conn))
                .service(scope("/").service(super::get)),
        )
        .await;

        let req = TestRequest::get().uri("/").to_request();
        let res: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.as_array().unwrap().len(), 0);

        Ok(())
    }

    #[actix_web::test]
    async fn get_one_row() -> Result<()> {
        let conn = db::setup_connection()?;

        User::insert(1, &OsmUserJson::mock(), &conn)?;

        let app = test::init_service(
            App::new()
                .app_data(Data::new(conn))
                .service(scope("/").service(super::get)),
        )
        .await;

        let req = TestRequest::get().uri("/").to_request();
        let res: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.as_array().unwrap().len(), 1);

        Ok(())
    }

    #[actix_web::test]
    async fn get_updated_since() -> Result<()> {
        let conn = db::setup_connection()?;

        conn.execute(
            "INSERT INTO user (rowid, osm_json, updated_at) VALUES (1, json(?), '2022-01-05T00:00:00Z')",
            [serde_json::to_string(&OsmUserJson::mock())?],
        )?;
        conn.execute(
            "INSERT INTO user (rowid, osm_json, updated_at) VALUES (2, json(?), '2022-02-05T00:00:00Z')",
            [serde_json::to_string(&OsmUserJson::mock())?],
        )?;

        let app = test::init_service(
            App::new()
                .app_data(Data::new(conn))
                .service(scope("/").service(super::get)),
        )
        .await;

        let req = TestRequest::get()
            .uri("/?updated_since=2022-01-10")
            .to_request();
        let res: Vec<GetItem> = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.len(), 1);

        Ok(())
    }

    #[actix_web::test]
    async fn get_by_id() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        db::migrate(&mut conn)?;

        let user_id = 1;
        User::insert(user_id, &OsmUserJson::mock(), &conn)?;

        let app = test::init_service(
            App::new()
                .app_data(Data::new(conn))
                .service(super::get_by_id),
        )
        .await;
        let req = TestRequest::get().uri(&format!("/{user_id}")).to_request();
        let res: GetItem = test::call_and_read_body_json(&app, req).await;
        assert_eq!(res.id, user_id);

        Ok(())
    }

    #[actix_web::test]
    async fn patch_tags() -> Result<()> {
        let mut conn = Connection::open_in_memory()?;
        db::migrate(&mut conn)?;

        let admin_token = "test";
        conn.execute(
            token::INSERT,
            named_params! { ":user_id": 1, ":secret": admin_token },
        )?;

        let user_id = 1;
        User::insert(user_id, &OsmUserJson::mock(), &conn)?;

        let app = test::init_service(
            App::new()
                .app_data(Data::new(conn))
                .service(super::patch_tags),
        )
        .await;

        let req = TestRequest::patch()
            .uri(&format!("/{user_id}/tags"))
            .append_header(("Authorization", format!("Bearer {admin_token}")))
            .set_json(json!({ "foo": "bar" }))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(res.status(), StatusCode::OK);

        Ok(())
    }
}
