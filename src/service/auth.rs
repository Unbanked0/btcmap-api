use crate::{model::token, ApiError};
use actix_web::HttpRequest;
use rusqlite::{Connection, OptionalExtension};
use token::Token;

pub fn get_admin_token(db: &Connection, req: &HttpRequest) -> Result<Token, ApiError> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .map(|it| it.to_str().unwrap_or(""))
        .unwrap_or("");

    if auth_header.len() == 0 {
        return Err(ApiError::new(401, "Authorization header is missing"));
    }

    let auth_header_parts: Vec<&str> = auth_header.split(" ").collect();
    if auth_header_parts.len() != 2 {
        return Err(ApiError::new(401, "Authorization header is invalid"));
    }

    let secret = auth_header_parts[1];

    let token = db
        .query_row(
            token::SELECT_BY_SECRET,
            &[(":secret", &secret)],
            token::SELECT_BY_SECRET_MAPPER,
        )
        .optional()?;

    match token {
        Some(token) => {
            return Ok(token);
        }
        None => {
            return Err(ApiError::new(401, "Invalid token"));
        }
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::{named_params, Connection};

    use super::*;
    use crate::command::db;
    use actix_web::{
        dev::Response,
        get,
        test::{self, TestRequest},
        web::{scope, Data},
        App, Responder,
    };

    #[actix_web::test]
    async fn no_header() {
        let db = db::tests::db().unwrap();

        db.execute(
            token::INSERT,
            named_params! {
                ":user_id": "1",
                ":secret": "test",
            },
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::new(db))
                .service(scope("/").service(get)),
        )
        .await;
        let req = TestRequest::get().uri("/").to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(401, res.status().as_u16())
    }

    #[actix_web::test]
    async fn valid_token() {
        let db = db::tests::db().unwrap();

        db.execute(
            token::INSERT,
            named_params! {
                ":user_id": 1,
                ":secret": "qwerty",
            },
        )
        .unwrap();

        let app = test::init_service(
            App::new()
                .app_data(Data::new(Connection::open(db.path().unwrap()).unwrap()))
                .service(scope("/").service(get)),
        )
        .await;
        let req = TestRequest::get()
            .uri("/")
            .append_header(("Authorization", "Bearer qwerty"))
            .to_request();
        let res = test::call_service(&app, req).await;
        assert_eq!(200, res.status().as_u16())
    }

    #[get("")]
    async fn get(req: HttpRequest, db: Data<Connection>) -> Result<impl Responder, ApiError> {
        get_admin_token(&db, &req)?;
        Ok(Response::ok())
    }
}
