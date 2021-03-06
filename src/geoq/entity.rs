extern crate geo_types;
extern crate geohash;
extern crate geojson;
extern crate regex;
extern crate geoq_wkt;

use geo_types::{Geometry, LineString, Point, Polygon};
use geoq::error::Error;
use geojson::conversion::*;
use geojson::GeoJson;
use geoq::input::Input;
use regex::Regex;
use serde_json;
use geoq_wkt::ToGeo;
use geoq_wkt::ToWkt;
use std::fmt;

lazy_static! {
    static ref LATLON_SPLIT: Regex = Regex::new(",|\t").unwrap();
}

pub enum Entity {
    LatLon(String),
    Geohash(String),
    Wkt(String, geoq_wkt::Geometry<f64>),
    GeoJsonFeature(String, geojson::Feature),
    GeoJsonGeometry(String, geojson::Geometry),
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Entity::LatLon(ref raw) => write!(f, "LatLon: {}", raw),
            Entity::Geohash(ref raw) => write!(f, "Geohash: {}", raw),
            Entity::Wkt(ref raw, _) => write!(f, "WKT: {}", raw),
            Entity::GeoJsonFeature(ref raw, _) => write!(f, "GeoJSON Feature: {}", raw),
            Entity::GeoJsonGeometry(ref raw, _) => write!(f, "GeoJSON Geometry: {}", raw),
        }
    }
}

fn latlon_geom(raw: &String) -> geo_types::Geometry<f64> {
    let pieces = LATLON_SPLIT.split(raw).collect::<Vec<&str>>();
    let lat = pieces[0].parse::<f64>().unwrap();
    let lon = pieces[1].parse::<f64>().unwrap();
    Geometry::Point(Point::new(lon, lat))
}

fn geohash_geom(raw: &String) -> geo_types::Geometry<f64> {
    let (bl, tr) = geohash::decode_bbox(raw);
    let outer = LineString(vec![
        Point::new(bl.x, bl.y),
        Point::new(tr.x, bl.y),
        Point::new(tr.x, tr.y),
        Point::new(bl.x, tr.y),
        Point::new(bl.x, bl.y),
    ]);
    Geometry::Polygon(Polygon::new(outer, Vec::new()))
}

fn wkt_entities(raw: &String) -> Result<Vec<Entity>, Error> {
    let wkt_res: Result<geoq_wkt::Wkt<f64>, &str> = geoq_wkt::Wkt::from_str(&raw);
    let mut entities = Vec::new();
    match wkt_res {
        Ok(wkts) => {
            for wkt_geom in wkts.items {
                let wkt_raw = wkt_geom.to_string();
                entities.push(Entity::Wkt(wkt_raw, wkt_geom))
            }
        },
        Err(_e) => {
            return Err(Error::InvalidWkt)
        }
    }
    Ok(entities)
}

fn parsed_geojson_entities(raw: String, gj: GeoJson) -> Vec<Entity> {
    match gj {
        GeoJson::Geometry(gj_geom) => vec![Entity::GeoJsonGeometry(raw, gj_geom)],
        GeoJson::Feature(gj_feature) => vec![Entity::GeoJsonFeature(raw, gj_feature)],
        GeoJson::FeatureCollection(gj_fc) => gj_fc
            .features
            .into_iter()
            .map(|f| {
                let gj_raw = serde_json::to_string(&f).unwrap();
                Entity::GeoJsonFeature(gj_raw, f)
            })
            .collect(),
    }
}

fn geojson_entities(raw: String) -> Result<Vec<Entity>, Error> {
    match raw.parse() {
        Ok(gj) => Ok(parsed_geojson_entities(raw, gj)),
        Err(e) => {
            eprintln!("Error parsing geojson: {} - {}", raw, e);
            Err(Error::InvalidGeoJSON)
        }
    }
}

impl Entity {
    pub fn geom(self) -> geo_types::Geometry<f64> {
        match self {
            Entity::LatLon(ref raw) => latlon_geom(raw),
            Entity::Geohash(ref raw) => geohash_geom(raw),
            Entity::Wkt(_, ref wkt_geom) => wkt_geom.to_geo().unwrap(),
            Entity::GeoJsonGeometry(_, gj_geom) => gj_geom.value.try_into().unwrap(),
            Entity::GeoJsonFeature(_, gj_feature) => {
                gj_feature.geometry.unwrap().value.try_into().unwrap()
            }
        }
    }

    pub fn wkt(self) -> geoq_wkt::Geometry<f64> {
        let geom = self.geom();
        let mut wkt = geom.to_wkt();
        wkt.items.pop().unwrap()
    }

    pub fn geojson_geometry(self) -> geojson::Geometry {
        let geom = self.geom();
        geojson::Geometry::new(geojson::Value::from(&geom))
    }

    pub fn geojson_properties(&self) -> serde_json::Map<String, serde_json::value::Value> {
        match *self {
            Entity::GeoJsonFeature(_, ref f) => {
                if let Some(props) = &f.properties {
                    props.clone()
                } else {
                    serde_json::Map::new()
                }
            }
            _ => serde_json::Map::new(),
        }
    }

    pub fn geojson_feature(self) -> geojson::Feature {
        let props = self.geojson_properties();
        let geom = self.geojson_geometry();
        geojson::Feature {
            bbox: None,
            geometry: Some(geom),
            id: None,
            properties: Some(props),
            foreign_members: None,
        }
    }

    pub fn raw(&self) -> String {
        match *self {
            Entity::LatLon(ref raw) => raw.clone(),
            Entity::Geohash(ref raw) => raw.clone(),
            Entity::Wkt(ref raw, _) => raw.clone(),
            Entity::GeoJsonGeometry(ref raw, _) => raw.clone(),
            Entity::GeoJsonFeature(ref raw, _) => raw.clone(),
        }
    }
}

pub fn from_input(i: Input) -> Result<Vec<Entity>, Error> {
    match i {
        Input::LatLon(raw) => Ok(vec![Entity::LatLon(raw)]),
        Input::Geohash(raw) => Ok(vec![Entity::Geohash(raw)]),
        Input::WKT(raw) => wkt_entities(&raw),
        Input::GeoJSON(raw) => geojson_entities(raw)
    }
}

#[cfg(test)]
mod tests {
    extern crate serde_json;
    extern crate geoq_wkt;

    use geo_types::{Geometry, LineString, Point, Polygon};
    use geoq::entity::{self, Entity};
    use geoq::input::Input;
    use serde_json::{Map as JMap};
    use serde_json::value::{Value as JValue};

    fn entities(i: &Input) -> Vec<Entity> {
        entity::from_input(i.clone()).expect(&format!("Should get entities from input {}", i))
    }

    fn check(input: Input,
             exp_raw: Vec<&str>,
             exp_geoms: Vec<Geometry<f64>>,
             exp_wkts: Vec<&str>,
             exp_gj_geoms: Vec<&str>,
             exp_gj_properties: Vec<JMap<String, JValue>>,
             exp_gj_features: Vec<&str>) {

        let res_raw: Vec<String> = entities(&input).iter().map(|e| e.raw()).collect();
        assert_eq!(exp_raw, res_raw);

        let res_raw: Vec<String> = entities(&input).iter().map(|e| e.raw()).collect();
        assert_eq!(exp_raw, res_raw);

        let res_geoms: Vec<Geometry<f64>> = entities(&input).into_iter().map(|e| e.geom()).collect();
        assert_eq!(exp_geoms, res_geoms);

        let res_wkts: Vec<String> = entities(&input).into_iter().map(|e| e.wkt().to_string()).collect();
        assert_eq!(exp_wkts, res_wkts);

        let res_gj_geoms: Vec<String> = entities(&input)
            .into_iter()
            .map(|e| e.geojson_geometry())
            .map(|ref gj| serde_json::to_string(gj).unwrap())
            .collect();
        assert_eq!(exp_gj_geoms, res_gj_geoms);

        let res_gj_properties: Vec<JMap<String, JValue>> = entities(&input).into_iter().map(|e| e.geojson_properties()).collect();
        assert_eq!(exp_gj_properties, res_gj_properties);

        let res_gj_features: Vec<String> = entities(&input)
            .into_iter()
            .map(|e| e.geojson_feature())
            .map(|ref gj| serde_json::to_string(gj).unwrap())
            .collect();
        assert_eq!(exp_gj_features, res_gj_features);
    }

    #[test]
    fn entities_for_latlon() {
        check(Input::LatLon("12,34".to_string()),
              vec!["12,34"],
              vec![Geometry::Point(Point::new(34.0, 12.0))],
              vec!["POINT(34 12)"],
              vec!["{\"coordinates\":[34.0,12.0],\"type\":\"Point\"}"],
              vec![serde_json::Map::new()],
              vec!["{\"geometry\":{\"coordinates\":[34.0,12.0],\"type\":\"Point\"},\"properties\":{},\"type\":\"Feature\"}"]
        );
    }

    #[test]
    fn entities_for_geohash() {
        let exp_poly = Polygon::new(
            vec![
                [-119.53125, 33.75],
                [-118.125, 33.75],
                [-118.125, 35.15625],
                [-119.53125, 35.15625],
                [-119.53125, 33.75],
            ].into(),
            vec![],
        );
        check(Input::Geohash("9q5".to_string()),
              vec!["9q5"],
              vec![Geometry::Polygon(exp_poly)],
              vec!["POLYGON((-119.53125 33.75,-118.125 33.75,-118.125 35.15625,-119.53125 35.15625,-119.53125 33.75))"],
              vec!["{\"coordinates\":[[[-119.53125,33.75],[-118.125,33.75],[-118.125,35.15625],[-119.53125,35.15625],[-119.53125,33.75]]],\"type\":\"Polygon\"}"],
              vec![serde_json::Map::new()],
              vec!["{\"geometry\":{\"coordinates\":[[[-119.53125,33.75],[-118.125,33.75],[-118.125,35.15625],[-119.53125,35.15625],[-119.53125,33.75]]],\"type\":\"Polygon\"},\"properties\":{},\"type\":\"Feature\"}"]);
    }

    #[test]
    fn entities_for_wkt() {
        let exp_geom = LineString(
            vec![
                Point::new(30.0, 10.0),
                Point::new(10.0, 30.0),
                Point::new(40.0, 40.0),
            ].into(),
        );
        check(Input::WKT("LINESTRING (30 10, 10 30, 40 40)".to_string()),
              vec!["LINESTRING(30 10,10 30,40 40)"],
              vec![Geometry::LineString(exp_geom)],
              vec!["LINESTRING(30 10,10 30,40 40)"],
              vec!["{\"coordinates\":[[30.0,10.0],[10.0,30.0],[40.0,40.0]],\"type\":\"LineString\"}"],
              vec![serde_json::Map::new()],
              vec!["{\"geometry\":{\"coordinates\":[[30.0,10.0],[10.0,30.0],[40.0,40.0]],\"type\":\"LineString\"},\"properties\":{},\"type\":\"Feature\"}"]);
    }

    #[test]
    fn entities_for_geojson_geometry() {
        let exp_geom = LineString(
            vec![
                Point::new(-26.01, 59.17),
                Point::new(-15.46, 45.58),
                Point::new(0.35, 35.74),
            ].into(),
        );
        let raw = "{\"type\": \"LineString\", \"coordinates\": [[-26.01, 59.17], [-15.46, 45.58], [0.35, 35.74]]}";
        check(Input::GeoJSON(raw.to_string()),
              vec!["{\"type\": \"LineString\", \"coordinates\": [[-26.01, 59.17], [-15.46, 45.58], [0.35, 35.74]]}"],
              vec![Geometry::LineString(exp_geom)],
              vec!["LINESTRING(-26.01 59.17,-15.46 45.58,0.35 35.74)"],
              vec!["{\"coordinates\":[[-26.01,59.17],[-15.46,45.58],[0.35,35.74]],\"type\":\"LineString\"}"],
              vec![serde_json::Map::new()],
              vec!["{\"geometry\":{\"coordinates\":[[-26.01,59.17],[-15.46,45.58],[0.35,35.74]],\"type\":\"LineString\"},\"properties\":{},\"type\":\"Feature\"}"]
        )
    }

    #[test]
    fn entities_for_geojson_feature() {
        // TODO - make properties map optional for geojson inputs?
        let raw = "{\"type\": \"Feature\", \"properties\": {\"pizza\": \"pie\"}, \"geometry\": {\"type\": \"LineString\", \"coordinates\": [[-26.01, 59.17], [-15.46, 45.58], [0.35, 35.74]]}}";
        let i = Input::GeoJSON(raw.to_string());
        let exp_geom = LineString(
            vec![
                Point::new(-26.01, 59.17),
                Point::new(-15.46, 45.58),
                Point::new(0.35, 35.74),
            ].into(),
        );

        let mut exp_properties = serde_json::Map::new();
        exp_properties.insert(String::from("pizza"), serde_json::to_value("pie").unwrap());
        check(i,
              vec![raw],
              vec![Geometry::LineString(exp_geom)],
              vec!["LINESTRING(-26.01 59.17,-15.46 45.58,0.35 35.74)"],
              vec!["{\"coordinates\":[[-26.01,59.17],[-15.46,45.58],[0.35,35.74]],\"type\":\"LineString\"}"],
              vec![exp_properties],
              vec!["{\"geometry\":{\"coordinates\":[[-26.01,59.17],[-15.46,45.58],[0.35,35.74]],\"type\":\"LineString\"},\"properties\":{\"pizza\":\"pie\"},\"type\":\"Feature\"}"]
        )
    }

    #[test]
    fn entities_for_geojson_feature_collection() {
        let raw = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"a":"b"},"geometry":{"type":"Point","coordinates":[34.0,12.0]}},{"type":"Feature","properties":{"c":1},"geometry":{"type":"Point","coordinates":[78.0,56.0]}}]}"#;
        let i = Input::GeoJSON(raw.to_string());
        let geoms = vec![Geometry::Point(Point::new(34.0, 12.0)),
                         Geometry::Point(Point::new(78.0, 56.0))];
        let mut props1 = serde_json::Map::new();
        props1.insert(String::from("a"), serde_json::to_value("b").unwrap());
        let mut props2 = serde_json::Map::new();
        props2.insert(String::from("c"), serde_json::to_value(1).unwrap());
        check(i,
              vec![
                  "{\"geometry\":{\"coordinates\":[34.0,12.0],\"type\":\"Point\"},\"properties\":{\"a\":\"b\"},\"type\":\"Feature\"}",
                  "{\"geometry\":{\"coordinates\":[78.0,56.0],\"type\":\"Point\"},\"properties\":{\"c\":1},\"type\":\"Feature\"}"
              ],
              geoms,
              vec!["POINT(34 12)", "POINT(78 56)"],
              vec![
                  "{\"coordinates\":[34.0,12.0],\"type\":\"Point\"}",
                  "{\"coordinates\":[78.0,56.0],\"type\":\"Point\"}",
              ],
              vec![props1, props2],
              vec![
                  "{\"geometry\":{\"coordinates\":[34.0,12.0],\"type\":\"Point\"},\"properties\":{\"a\":\"b\"},\"type\":\"Feature\"}",
                  "{\"geometry\":{\"coordinates\":[78.0,56.0],\"type\":\"Point\"},\"properties\":{\"c\":1},\"type\":\"Feature\"}"
              ]
        )
    }
}
