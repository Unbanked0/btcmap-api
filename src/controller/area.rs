use crate::db;
use crate::model::ApiError;
use crate::model::Area;
use crate::model::Element;
use actix_web::get;
use actix_web::web::Data;
use actix_web::web::Json;
use actix_web::web::Path;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Mutex;
use time::Duration;
use time::OffsetDateTime;

use std::ops::Sub;

#[derive(Serialize, Deserialize)]
pub struct GetAreasItem {
    pub id: String,
    pub name: String,
    pub area_type: String,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
    pub elements: usize,
    pub up_to_date_elements: usize,
}

#[get("/areas")]
async fn get_areas(conn: Data<Mutex<Connection>>) -> Result<Json<Vec<GetAreasItem>>, ApiError> {
    let conn = conn.lock()?;

    let areas: Vec<Area> = conn
        .prepare(db::AREA_SELECT_ALL)?
        .query_map([], db::mapper_area_full())?
        .filter(|it| it.is_ok())
        .map(|it| it.unwrap())
        .collect();

    let elements: Vec<Element> = conn
        .prepare(db::ELEMENT_SELECT_ALL)?
        .query_map([], db::mapper_element_full())?
        .filter(|it| it.is_ok())
        .map(|it| it.unwrap())
        .collect();

    let mut res: Vec<GetAreasItem> = vec![];
    let today = OffsetDateTime::now_utc().date();
    let year_ago = today.sub(Duration::days(365));

    for area in areas {
        let area_elements: Vec<&Element> = elements
            .iter()
            .filter(|it| it.data["type"].as_str().unwrap() == "node")
            .filter(|it| {
                let lat = it.data["lat"].as_f64().unwrap();
                let lon = it.data["lon"].as_f64().unwrap();
                lon > area.min_lon && lon < area.max_lon && lat > area.min_lat && lat < area.max_lat
            })
            .collect();

        let elements_len = area_elements.len();

        let up_to_date_elements: Vec<&Element> = area_elements
            .into_iter()
            .filter(|it| {
                (it.data["tags"].get("survey:date").is_some()
                    && it.data["tags"]["survey:date"].as_str().unwrap().to_string()
                        > year_ago.to_string())
                    || (it.data["tags"].get("check_date").is_some()
                        && it.data["tags"]["check_date"].as_str().unwrap().to_string()
                            > year_ago.to_string())
            })
            .collect();

        res.push(GetAreasItem {
            id: area.id,
            name: area.name,
            area_type: area.area_type,
            min_lon: area.min_lon,
            min_lat: area.min_lat,
            max_lon: area.max_lon,
            max_lat: area.max_lat,
            elements: elements_len,
            up_to_date_elements: up_to_date_elements.len(),
        });
    }

    Ok(Json(res))
}

#[get("/areas/{id}")]
async fn get_area(
    path: Path<String>,
    conn: Data<Mutex<Connection>>,
) -> Result<Json<GetAreasItem>, ApiError> {
    let id_or_name = path.into_inner();
    let conn = conn.lock()?;

    let area_by_id = conn
        .query_row(db::AREA_SELECT_BY_ID, [&id_or_name], db::mapper_area_full())
        .optional()?;

    match area_by_id {
        Some(area) => area_to_areas_item(area, &conn),
        None => {
            let area_by_name = conn
                .query_row(
                    db::AREA_SELECT_BY_NAME,
                    [&id_or_name],
                    db::mapper_area_full(),
                )
                .optional()?;

            match area_by_name {
                Some(area) => area_to_areas_item(area, &conn),
                None => Result::Err(ApiError {
                    message: format!("Area with id or name {} doesn't exist", &id_or_name)
                        .to_string(),
                }),
            }
        }
    }
}

fn area_to_areas_item(area: Area, conn: &Connection) -> Result<Json<GetAreasItem>, ApiError> {
    let all_elements: Vec<Element> = conn
        .prepare(db::ELEMENT_SELECT_ALL)?
        .query_map([], db::mapper_element_full())?
        .map(|row| row.unwrap())
        .collect();

    let area_elements: Vec<&Element> = all_elements
        .iter()
        .filter(|it| {
            it.lon() > area.min_lon
                && it.lon() < area.max_lon
                && it.lat() > area.min_lat
                && it.lat() < area.max_lat
        })
        .collect();

    let elements_len = area_elements.len();
    let today = OffsetDateTime::now_utc().date();
    let year_ago = today.sub(Duration::days(365));

    let up_to_date_elements: Vec<&Element> = area_elements
        .into_iter()
        .filter(|it| {
            (it.data["tags"].get("survey:date").is_some()
                && it.data["tags"]["survey:date"].as_str().unwrap().to_string()
                    > year_ago.to_string())
                || (it.data["tags"].get("check_date").is_some()
                    && it.data["tags"]["check_date"].as_str().unwrap().to_string()
                        > year_ago.to_string())
        })
        .collect();

    Ok(Json(GetAreasItem {
        id: area.id,
        name: area.name,
        area_type: area.area_type,
        min_lon: area.min_lon,
        min_lat: area.min_lat,
        max_lon: area.max_lon,
        max_lat: area.max_lat,
        elements: elements_len,
        up_to_date_elements: up_to_date_elements.len(),
    }))
}

#[get("/areas/{id}/elements")]
async fn get_area_elements(
    path: Path<String>,
    conn: Data<Mutex<Connection>>,
) -> Result<Json<Vec<Element>>, ApiError> {
    let conn = conn.lock()?;

    let area = conn
        .query_row(
            db::AREA_SELECT_BY_ID,
            [path.into_inner()],
            db::mapper_area_full(),
        )
        .optional()
        .unwrap();

    if let None = area {
        return Ok(Json(vec![]));
    }

    let area = area.unwrap();

    let elements: Vec<Element> = conn
        .prepare(db::ELEMENT_SELECT_ALL)?
        .query_map([], db::mapper_element_full())?
        .filter(|it| it.is_ok())
        .map(|it| it.unwrap())
        .filter(|it| {
            let element_type = it.data["type"].as_str().unwrap();

            if element_type != "node" {
                return false;
            }

            let lat = it.data["lat"].as_f64().unwrap();
            let lon = it.data["lon"].as_f64().unwrap();

            lon > area.min_lon && lon < area.max_lon && lat > area.min_lat && lat < area.max_lat
        })
        .collect();

    Ok(Json(elements))
}