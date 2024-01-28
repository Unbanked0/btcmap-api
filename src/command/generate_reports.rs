use crate::area::Area;
use crate::osm::overpass;
use crate::osm::overpass::OverpassElement;
use crate::report::Report;
use crate::Result;
use geo::Contains;
use geo::LineString;
use geo::MultiPolygon;
use geo::Polygon;
use geojson::GeoJson;
use geojson::Geometry;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use time::format_description::well_known::Iso8601;
use time::OffsetDateTime;
use tracing::error;
use tracing::info;

pub async fn run(mut conn: Connection) -> Result<()> {
    let today = OffsetDateTime::now_utc().date();
    info!(date = ?today, "Generating report");

    let existing_reports = Report::select_updated_since(&today.to_string(), None, &conn)?;

    if !existing_reports.is_empty() {
        info!("Found existing reports, aborting");
        return Ok(());
    }

    let elements = overpass::query_bitcoin_merchants().await?;

    let areas: Vec<Area> = Area::select_all(None, &conn)?
        .into_iter()
        .filter(|it| it.deleted_at == None)
        .collect();

    let tx = conn.transaction()?;

    for area in areas {
        info!(area.id, "Generating report");

        if area.tags.get("url_alias") == Some(&Value::String("earth".into())) {
            info!(area.id, elements = elements.len(), "Processing area");
            let report_tags = generate_report_tags(&elements.iter().collect::<Vec<_>>())?;
            insert_report(area.id, &report_tags, &tx).await?;
            continue;
        }

        let mut area_elements: Vec<&OverpassElement> = vec![];
        let geo_json = area.tags.get("geo_json").unwrap_or(&Value::Null);

        if geo_json.is_null() {
            continue;
        }

        if geo_json.is_object() {
            let geo_json: Result<GeoJson, _> = serde_json::to_string(geo_json)?.parse();

            let geo_json = match geo_json {
                Ok(geo_json) => geo_json,
                Err(e) => {
                    error!(?e, "Failed to parse GeoJSON");
                    continue;
                }
            };

            let mut geometries: Vec<&Geometry> = vec![];

            match &geo_json {
                GeoJson::FeatureCollection(v) => {
                    for feature in &v.features {
                        if let Some(v) = &feature.geometry {
                            geometries.push(v);
                        }
                    }
                }
                GeoJson::Feature(v) => {
                    if let Some(v) = &v.geometry {
                        geometries.push(v);
                    }
                }
                GeoJson::Geometry(v) => geometries.push(v),
            };

            for element in &elements {
                for geometry in &geometries {
                    match &geometry.value {
                        geojson::Value::MultiPolygon(_) => {
                            let multi_poly: MultiPolygon = (&geometry.value).try_into().unwrap();

                            if multi_poly.contains(&element.coord()) {
                                area_elements.push(element);
                            }
                        }
                        geojson::Value::Polygon(_) => {
                            let poly: Polygon = (&geometry.value).try_into().unwrap();

                            if poly.contains(&element.coord()) {
                                area_elements.push(element);
                            }
                        }
                        geojson::Value::LineString(_) => {
                            let line_string: LineString = (&geometry.value).try_into().unwrap();

                            if line_string.contains(&element.coord()) {
                                area_elements.push(element);
                            }
                        }
                        _ => continue,
                    }
                }
            }
        }

        info!(area.id, elements = area_elements.len(), "Processing area");
        let report_tags = generate_report_tags(&area_elements)?;
        insert_report(area.id, &report_tags, &tx).await?;
    }

    tx.commit()?;

    Ok(())
}

fn generate_report_tags(elements: &[&OverpassElement]) -> Result<HashMap<String, Value>> {
    info!("Generating report tags");

    let atms: Vec<_> = elements
        .iter()
        .filter(|it| it.tag("amenity") == "atm")
        .collect();

    let onchain_elements: Vec<_> = elements
        .iter()
        .filter(|it| it.tag("payment:onchain") == "yes")
        .collect();

    let lightning_elements: Vec<_> = elements
        .iter()
        .filter(|it| it.tag("payment:lightning") == "yes")
        .collect();

    let lightning_contactless_elements: Vec<_> = elements
        .iter()
        .filter(|it| it.tag("payment:lightning_contactless") == "yes")
        .collect();

    let legacy_elements: Vec<_> = elements
        .iter()
        .filter(|it| it.tag("payment:bitcoin") == "yes")
        .collect();

    let up_to_date_elements: Vec<_> = elements.iter().filter(|it| it.up_to_date()).collect();

    let outdated_elements: Vec<_> = elements
        .iter()
        .filter(|it| !it.up_to_date())
        .copied()
        .collect();

    let up_to_date_percent: f64 = up_to_date_elements.len() as f64 / elements.len() as f64 * 100.0;
    let up_to_date_percent: i64 = up_to_date_percent as i64;

    let mut tags: HashMap<String, Value> = HashMap::new();
    tags.insert("total_elements".into(), elements.len().into());
    tags.insert("total_atms".into(), atms.len().into());
    tags.insert(
        "total_elements_onchain".into(),
        onchain_elements.len().into(),
    );
    tags.insert(
        "total_elements_lightning".into(),
        lightning_elements.len().into(),
    );
    tags.insert(
        "total_elements_lightning_contactless".into(),
        lightning_contactless_elements.len().into(),
    );
    tags.insert(
        "up_to_date_elements".into(),
        up_to_date_elements.len().into(),
    );
    tags.insert("outdated_elements".into(), outdated_elements.len().into());
    tags.insert("legacy_elements".into(), legacy_elements.len().into());
    tags.insert(
        "up_to_date_percent".into(),
        (up_to_date_percent as usize).into(),
    );

    let now = OffsetDateTime::now_utc();

    let verification_dates: Vec<i64> = elements
        .iter()
        .filter_map(|it| it.verification_date().map(|it| it.unix_timestamp()))
        .filter_map(|it| {
            if it > now.unix_timestamp() {
                None
            } else {
                Some(it)
            }
        })
        .collect();

    if verification_dates.len() > 0 {
        let avg_verification_date: f64 =
            verification_dates.iter().sum::<i64>() as f64 / verification_dates.len() as f64;
        let avg_verification_date: i64 = avg_verification_date as i64;
        let avg_verification_date = OffsetDateTime::from_unix_timestamp(avg_verification_date);

        if let Ok(avg_verification_date) = avg_verification_date {
            tags.insert(
                "avg_verification_date".into(),
                avg_verification_date
                    .format(&Iso8601::DEFAULT)
                    .unwrap()
                    .into(),
            );
        }
    }

    Ok(tags)
}

async fn insert_report(
    area_id: i64,
    tags: &HashMap<String, Value>,
    conn: &Connection,
) -> Result<()> {
    let date = OffsetDateTime::now_utc().date();
    info!(area_id, ?date, ?tags, "Inserting new report");
    Report::insert(area_id, &date, &tags, conn)?;
    info!(area_id, ?date, "Inserted new report");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::mock_state;
    use serde_json::{json, Map};
    use time::{macros::date, Duration};
    use tokio::test;

    #[test]
    async fn insert_report() -> Result<()> {
        let state = mock_state().await;
        let mut area_tags = Map::new();
        area_tags.insert("url_alias".into(), json!("test"));
        state.area_repo.insert(&area_tags).await?;
        for _ in 1..100 {
            state
                .report_repo
                .insert(1, &date!(2023 - 11 - 12), &HashMap::new())
                .await?;
        }
        Ok(())
    }

    #[test]
    async fn generate_report_tags() -> Result<()> {
        let element_1 = json!({
          "type": "node",
          "id": 25338659,
          "lat": -33.9491670,
          "lon": 25.5640810,
          "timestamp": "2023-02-25T09:22:38Z",
          "version": 16,
          "changeset": 132997506,
          "user": "BTC Map",
          "uid": 18545877,
          "tags": {
            "addr:city": "Gqeberha",
            "addr:postcode": "6045",
            "addr:street": "4th Avenue",
            "air_conditioning": "yes",
            "branch": "Newton Park",
            "brand": "Pick n Pay",
            "brand:wikidata": "Q7190735",
            "brand:wikipedia": "en:Pick n Pay Stores",
            "check_date": "2023-02-15",
            "check_date:currency:XBT": "2023-02-25",
            "contact:website": "https://www.pnp.co.za/",
            "currency:XBT": "yes",
            "currency:ZAR": "yes",
            "diet:vegetarian": "yes",
            "level": "0",
            "name": "Pick n Pay",
            "payment:credit_cards": "yes",
            "payment:debit_cards": "yes",
            "payment:mastercard": "yes",
            "payment:visa": "yes",
            "phone": "+27 41 365 1268",
            "second_hand": "no",
            "shop": "supermarket",
            "source": "survey",
            "stroller": "yes",
            "wheelchair": "yes"
          }
        });

        let element_1: OverpassElement = serde_json::from_value(element_1)?;

        let element_2 = json!({
          "type": "node",
          "id": 6402700275 as i64,
          "lat": 28.4077730,
          "lon": -106.8668376,
          "timestamp": "2023-03-15T18:24:05Z",
          "version": 4,
          "changeset": 133721045,
          "user": "CENTSOARER",
          "uid": 232801,
          "tags": {
            "addr:city": "Cd. Cuauhtémoc",
            "addr:street": "Avenida Agustín Melgar",
            "brand": "Elektra",
            "brand:wikidata": "Q1142753",
            "brand:wikipedia": "es:Grupo Elektra",
            "check_date:currency:XBT": "2023-03-09",
            "currency:XBT": "yes",
            "name": "Elektra",
            "payment:cash": "yes",
            "payment:debit_cards": "yes",
            "payment:lightning": "yes",
            "payment:lightning_contactless": "no",
            "payment:onchain": "yes",
            "shop": "department_store",
            "website": "https://www.elektra.mx"
          }
        });

        let mut element_2: OverpassElement = serde_json::from_value(element_2)?;

        let today = OffsetDateTime::now_utc().date();
        let today_plus_year = today.checked_add(Duration::days(356)).unwrap();
        element_2.tags.as_mut().unwrap().insert(
            "check_date:currency:XBT".into(),
            today_plus_year.to_string().into(),
        );

        let report_tags = super::generate_report_tags(&vec![&element_1, &element_2])?;

        assert_eq!(2, report_tags["total_elements"].as_i64().unwrap());
        assert_eq!(
            "2023-02-25T00:00:00.000000000Z",
            report_tags["avg_verification_date"].as_str().unwrap(),
        );

        Ok(())
    }
}
