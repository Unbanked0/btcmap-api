use crate::model::element;
use crate::model::Element;
use crate::Connection;
use crate::Result;
use rusqlite::named_params;
use serde_json::Value;

pub async fn run(db: Connection) -> Result<()> {
    log::info!("Generating element categories");

    let elements: Vec<Element> = db
        .prepare(element::SELECT_ALL)?
        .query_map([], element::SELECT_ALL_MAPPER)?
        .collect::<Result<Vec<Element>, _>>()?
        .into_iter()
        .filter(|it| it.deleted_at.len() == 0)
        .collect();

    log::info!("Found {} elements", elements.len());

    let mut known = 0;
    let mut unknown = 0;

    for element in elements {
        let new_category_singular = element.category_singular();
        let old_category_singular = element.tags["category"].as_str().unwrap_or("");

        if new_category_singular != old_category_singular {
            log::info!(
                "Updating category for element {} ({old_category_singular} -> {new_category_singular})",
                element.id,
            );

            db.execute(
                element::INSERT_TAG,
                named_params! {
                    ":element_id": element.id,
                    ":tag_name": "$.category",
                    ":tag_value": new_category_singular,
                },
            )?;
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }

        if new_category_singular == "other" {
            unknown += 1;
        } else {
            known += 1;
        }

        let new_category_plural = element.category_plural();
        let old_category_plural = element.tags["category:plural"].as_str().unwrap_or("");

        if new_category_plural != old_category_plural {
            log::info!(
                "Updating category:plural for element {} ({old_category_plural} -> {new_category_plural})",
                element.id,
            );

            db.execute(
                element::INSERT_TAG,
                named_params! {
                    ":element_id": element.id,
                    ":tag_name": "$.category:plural",
                    ":tag_value": new_category_plural,
                },
            )?;
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
    }

    log::info!(
        "Finished generating categories. Known: {known}, unknown: {unknown}, coverage: {:.2}%",
        known as f64 / (known as f64 + unknown as f64) * 100.0
    );

    Ok(())
}

impl Element {
    pub fn category_singular(&self) -> String {
        let tags: &Value = &self.osm_json["tags"];

        let amenity = tags["amenity"].as_str().unwrap_or("");
        let tourism = tags["tourism"].as_str().unwrap_or("");

        let mut category: &str = "other";

        if amenity == "atm" {
            category = "atm";
        }

        if amenity == "cafe" {
            category = "cafe";
        }

        if amenity == "restaurant" {
            category = "restaurant";
        }

        if amenity == "bar" {
            category = "bar";
        }

        if amenity == "pub" {
            category = "pub";
        }

        if tourism == "hotel" {
            category = "hotel";
        }

        category.to_string()
    }

    pub fn category_plural(&self) -> String {
        let tags: &Value = &self.osm_json["tags"];

        let amenity = tags["amenity"].as_str().unwrap_or("");
        let tourism = tags["tourism"].as_str().unwrap_or("");

        let mut category: &str = "other";

        if amenity == "atm" {
            category = "atms";
        }

        if amenity == "cafe" {
            category = "cafes";
        }

        if amenity == "restaurant" {
            category = "restaurants";
        }

        if amenity == "bar" {
            category = "bars";
        }

        if amenity == "pub" {
            category = "pubs";
        }

        if tourism == "hotel" {
            category = "hotels";
        }

        category.to_string()
    }
}
