use std::{collections::HashMap, error::Error, ffi::OsString, fs::File};

use crate::{
    config::{COUNTRY_TO_MARKET, COUNTRY_TO_REGION, SECTORS, SECTOR_SYNONYMS},
    ImageFormat,
};
use plotly::{
    color::NamedColor,
    common::{HoverInfo, Marker, Title},
    layout::{Axis, GridPattern, LayoutGrid},
    Bar, ImageFormat as PlotlyImageFormat, Layout, Plot,
};
use strum::{Display, EnumIter};
use tracing::{error, event, instrument, Level};

const Y_AXIS_TITLE: &str = "% Net assets";

#[derive(Debug, Copy, Clone, EnumIter, Display, PartialEq, Eq)]
pub enum Exposure {
    Holding,
    Sector,
    Country,
    Region,
    Market,
}

#[derive(Debug, Default)]
pub struct Security {
    name: String,
    ter: f32,
    holding: HashMap<String, f32>,
    sector: HashMap<String, f32>,
    country: HashMap<String, f32>,
    region: HashMap<String, f32>,
    market: HashMap<String, f32>,
}

impl Security {
    fn get_exposure(&self, exposure: Exposure) -> &HashMap<String, f32> {
        match exposure {
            Exposure::Holding => &self.holding,
            Exposure::Sector => &self.sector,
            Exposure::Country => &self.country,
            Exposure::Region => &self.region,
            Exposure::Market => &self.market,
        }
    }

    fn get_exposure_mut(&mut self, exposure: Exposure) -> &mut HashMap<String, f32> {
        match exposure {
            Exposure::Holding => &mut self.holding,
            Exposure::Sector => &mut self.sector,
            Exposure::Country => &mut self.country,
            Exposure::Region => &mut self.region,
            Exposure::Market => &mut self.market,
        }
    }
}

pub struct Conf {
    pub limit: usize,
    pub currency: String,
    pub display: bool,
    pub image: bool,
    pub image_scale: f64,
    pub image_format: ImageFormat,
    pub output_file_name: OsString,
    pub output_folder: String,
}

type Record = HashMap<String, String>;

#[instrument(skip(file_path))]
pub fn parse_portfolio(
    file_path: &str,
) -> Result<(Option<f32>, HashMap<String, f32>), Box<dyn Error>> {
    let file = File::open(file_path)?;
    let mut errors = Vec::new();
    let mut portfolio = HashMap::<String, f32>::new();
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(file);
    let percent = {
        // We nest this call in its own scope because of lifetimes.
        let headers = rdr.headers()?.iter().collect::<Vec<_>>();
        if headers.contains(&"Weight") {
            event!(Level::TRACE, "Securities with weights");
            true
        } else if headers.contains(&"Amount") {
            event!(Level::TRACE, "Securities with total amounts");
            false
        } else {
            panic!("Bad CSV header {:?}", headers);
        }
    };
    let allocation_header = if percent { "Weight" } else { "Amount" };
    for result in rdr.deserialize() {
        let record: Record = result?;
        let isin = record.get("ISIN").unwrap();
        let allocation = record
            .get(allocation_header)
            .unwrap()
            .parse::<f32>()
            .unwrap();
        if percent && allocation > 100. {
            errors.push(format!(
                "Portfolio ISIN {} weight {} > 100%",
                isin, allocation
            ));
            continue;
        }
        portfolio.entry(isin.clone()).or_insert_with(|| allocation);
    }
    if !errors.is_empty() {
        for err in &errors {
            error!("{}", err);
        }
        panic!("Errors occured");
    }
    let total = if !percent {
        let total = portfolio.values().fold(0., |acc, v| acc + v);
        for val in portfolio.values_mut() {
            *val = *val / total;
        }
        event!(Level::INFO, "Portfolio total value {:.2}", total);
        Some(total)
    } else {
        for val in portfolio.values_mut() {
            *val /= 100.;
        }
        None
    };
    event!(
        Level::INFO,
        "Parsed {} securities into portfolio",
        portfolio.len()
    );
    event!(Level::TRACE, ?portfolio);
    Ok((total, portfolio))
}

#[instrument(skip(file_path))]
pub fn parse_securities(file_path: String) -> Result<HashMap<String, Security>, Box<dyn Error>> {
    let file = File::open(file_path)?;
    let mut securities = HashMap::<String, Security>::new();
    let mut rdr = csv::Reader::from_reader(file);
    let mut last_isin = String::new();
    for result in rdr.deserialize() {
        let record: Record = result?;
        let mut isin: String = record.get("ISIN").unwrap().to_string();
        if isin.is_empty() && !last_isin.is_empty() {
            isin = last_isin.clone();
        } else if !isin.is_empty() {
            last_isin = isin.clone();
        }
        let name = record.get("Name").unwrap();
        let ter = record.get("TER").unwrap().parse::<f32>().unwrap_or(0.);
        let holding = record.get("Holding").unwrap();
        let holding_weight = record
            .get("HoldingWeight")
            .unwrap()
            .parse::<f32>()
            .map(|v| v / 100.)
            .unwrap_or(0.);
        let mut sector = record.get("Sector").unwrap().clone();
        if !sector.is_empty() && !SECTORS.contains(sector.as_str()) {
            sector = SECTOR_SYNONYMS
                .get(sector.as_str())
                .ok_or(format!("Unknown sector {} in record {:?}", sector, record))?
                .clone()
                .to_string();
        }
        let sector_weight = record
            .get("SectorWeight")
            .unwrap()
            .parse::<f32>()
            .map(|v| v / 100.)
            .unwrap_or(0.);
        let country = record.get("Country").unwrap();
        let country_weight = record
            .get("CountryWeight")
            .unwrap()
            .parse::<f32>()
            .map(|v| v / 100.)
            .unwrap_or(0.);
        let region = record.get("Region").unwrap();
        let region_weight = record
            .get("RegionWeight")
            .unwrap()
            .parse::<f32>()
            .map(|v| v / 100.)
            .unwrap_or(0.);
        securities
            .entry(isin.clone().to_string())
            .and_modify(|security| {
                if !name.is_empty() {
                    security.name = name.clone();
                }
                if ter > 0.0 {
                    security.ter = ter;
                }
                if holding_weight > 0.0 {
                    security.holding.insert(holding.clone(), holding_weight);
                }
                if sector_weight > 0.0 {
                    security.sector.insert(sector.clone(), sector_weight);
                }
                if country_weight > 0.0 {
                    security.country.insert(country.clone(), country_weight);
                }
                if region_weight > 0.0 {
                    security.region.insert(region.clone(), region_weight);
                }
            })
            .or_insert_with(|| {
                let mut security = Security {
                    name: name.clone(),
                    ter,
                    ..Default::default()
                };
                if holding_weight > 0.0 {
                    security.holding.insert(holding.clone(), holding_weight);
                }
                if sector_weight > 0.0 {
                    security.sector.insert(sector.clone(), sector_weight);
                }
                if country_weight > 0.0 {
                    security.country.insert(country.clone(), country_weight);
                }
                if region_weight > 0.0 {
                    security.region.insert(region.clone(), region_weight);
                }
                security
            });
    }
    for (isin, security) in securities.iter_mut() {
        for (exposure, country_map) in [
            (Exposure::Region, &COUNTRY_TO_REGION),
            (Exposure::Market, &COUNTRY_TO_MARKET),
        ] {
            if security.get_exposure(exposure).is_empty() && !security.country.is_empty() {
                let security_countries = security.country.clone();
                for (country, weight) in security_countries.iter() {
                    let exp = country_map
                        .get(country.as_str())
                        .ok_or(format!("{} {} not defined", country, exposure))?
                        .clone()
                        .to_string();
                    security
                        .get_exposure_mut(exposure)
                        .entry(exp)
                        .and_modify(|v| *v += *weight)
                        .or_insert(*weight);
                }
                event!(
                    Level::TRACE,
                    "Calculated {} for {} [{}]: {:?}",
                    exposure,
                    isin,
                    security.name,
                    security.get_exposure(exposure)
                );
            }
        }
    }
    event!(
        Level::INFO,
        "Parsed {} securities into database",
        securities.len()
    );
    Ok(securities)
}

#[instrument(skip(securities, exposure, results, base_weight), name = "calc", fields(weight=base_weight))]
fn calc_exposure(
    securities: &HashMap<String, Security>,
    exposure: Exposure,
    isin: &str,
    base_weight: f32,
    results: &mut HashMap<String, f32>,
) -> Result<(), Box<dyn Error>> {
    event!(Level::TRACE, "Calculating exposure");
    let security = securities
        .get(isin)
        .ok_or(format!("ISIN {} not found in securities", isin))?;
    // First try to see if any of the holdings is actually an ETF/fund itself that would need expanding
    let holdings = security.get_exposure(Exposure::Holding);
    for (holding, weight) in holdings {
        if securities.contains_key(holding) {
            event!(
                Level::TRACE,
                "Recursing for holding {}, weight {}",
                holding,
                weight
            );
            calc_exposure(securities, exposure, holding, base_weight * weight, results)?;
            event!(
                Level::DEBUG,
                "Results after holding {}: {:?}",
                holding,
                results
            );
        }
    }
    let exposure_items = security.get_exposure(exposure);
    for (exposure_item, weight) in exposure_items.iter() {
        if exposure == Exposure::Holding && securities.contains_key(exposure_item) {
            continue;
        }
        event!(
            Level::TRACE,
            "{} exposure: {}->{}",
            exposure_item,
            weight,
            weight * base_weight
        );
        results
            .entry(exposure_item.to_owned())
            .and_modify(|v| {
                *v += weight * base_weight;
            })
            .or_insert_with(|| weight * base_weight);
    }
    Ok(())
}

pub fn analyze_exposure(
    securities: &HashMap<String, Security>,
    portfolio: &HashMap<String, f32>,
    exposure: Exposure,
) -> Result<Vec<(String, f32)>, Box<dyn Error>> {
    let mut results: HashMap<String, f32> = HashMap::new();
    let mut errors = Vec::new();
    for (isin, weight) in portfolio {
        let mut isin_results: HashMap<String, f32> = HashMap::new();
        let result = calc_exposure(securities, exposure, isin, *weight, &mut isin_results);
        match result {
            Ok(_) => {
                event!(Level::DEBUG, "Results for {}: {:?}", isin, isin_results);
                for (key, val) in isin_results.into_iter() {
                    results
                        .entry(key.clone())
                        .and_modify(|share| {
                            event!(
                                Level::TRACE,
                                "Modifying {}: {}->{}",
                                key,
                                *share,
                                *share + val
                            );
                            *share += val
                        })
                        .or_insert_with(|| val);
                }
            }
            Err(err) => {
                errors.push(err.to_string());
            }
        }
    }
    if !errors.is_empty() {
        for err in &errors {
            error!("{}", err);
        }
        panic!("Errors occured");
    }
    let mut results = results
        .into_iter()
        .map(|(k, v)| (k, v * 100.))
        .collect::<Vec<_>>();
    let total = results.iter().fold(0., |acc, (_, v)| acc + *v);
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    if total < 100. {
        results.push(("Unknown".to_string(), 100. - total));
    } else if total > 100. {
        panic!("Total {}% > 100%", total);
    }
    event!(Level::DEBUG, "Analysis results: {:?}", results);
    Ok(results)
}

#[instrument(skip_all, name = "calc")]
pub fn calculate_ter(
    securities: &HashMap<String, Security>,
    portfolio: &HashMap<String, f32>,
) -> Result<f32, Box<dyn Error>> {
    let mut ter = 0.0;
    for (isin, weight) in portfolio {
        let security = securities
            .get(isin)
            .ok_or(format!("ISIN {} not found in securities", isin))?;
        ter += security.ter * weight;
    }
    event!(Level::INFO, "Calculated portfolio TER: {:.3}%", ter);
    Ok(ter)
}

pub fn plot_grid(
    data: Vec<(Exposure, Vec<(String, f32)>)>,
    total: Option<f32>,
    ter: f32,
    conf: &Conf,
) -> Result<(), Box<dyn Error>> {
    let mut plot = Plot::new();

    let mut layout = Layout::new()
        .title(Title::new(
            format!(
                "Asset exposure for {} portfolio, TER {:.3}%",
                conf.output_file_name.to_string_lossy(),
                ter
            )
            .as_str(),
        ))
        .height(1024)
        .grid(
            LayoutGrid::new()
                .rows(data.len())
                .columns(1)
                .pattern(GridPattern::Independent),
        )
        .show_legend(false);
    for (idx, (exposure, data)) in data.into_iter().enumerate() {
        match idx {
            0 => {
                layout = layout
                    .x_axis(Axis::new().title(Title::new(exposure.to_string().as_str())))
                    .y_axis(Axis::new().title(Title::new(Y_AXIS_TITLE)));
            }
            1 => {
                layout = layout
                    .x_axis2(Axis::new().title(Title::new(exposure.to_string().as_str())))
                    .y_axis2(Axis::new().title(Title::new(Y_AXIS_TITLE)));
            }
            2 => {
                layout = layout
                    .x_axis3(Axis::new().title(Title::new(exposure.to_string().as_str())))
                    .y_axis3(Axis::new().title(Title::new(Y_AXIS_TITLE)));
            }
            3 => {
                layout = layout
                    .x_axis4(Axis::new().title(Title::new(exposure.to_string().as_str())))
                    .y_axis4(Axis::new().title(Title::new(Y_AXIS_TITLE)));
            }
            4 => {
                layout = layout
                    .x_axis5(Axis::new().title(Title::new(exposure.to_string().as_str())))
                    .y_axis5(Axis::new().title(Title::new(Y_AXIS_TITLE)));
            }
            _ => {}
        }
        let data = if data.len() > conf.limit {
            data.into_iter().take(conf.limit).collect()
        } else {
            data
        };
        let labels = data
            .iter()
            .map(|(v, _)| format!("{}", v.to_owned()))
            .collect::<Vec<_>>();
        let values = data.iter().map(|(_, v)| *v).collect::<Vec<_>>();

        if exposure == Exposure::Holding {
            let weights = values
                .iter()
                .map(|v| format!("{:.2}%", v))
                .collect::<Vec<_>>();
            let mut trace = Bar::new(labels, values.clone())
                .hover_info(HoverInfo::None)
                .text_array(weights)
                .name("")
                .marker(Marker::new())
                .x_axis(format!("x{}", idx + 1))
                .y_axis(format!("y{}", idx + 1));
            if let Some(total) = total {
                let totals = values
                    .iter()
                    .map(|v| format!("{:.0} {}", *v * total / 100., conf.currency))
                    .collect::<Vec<_>>();
                trace = trace
                    .hover_info(HoverInfo::Text)
                    .hover_template_array(totals);
            }
            plot.add_trace(trace);
        } else {
            for (k, v) in data.into_iter() {
                let mut trace = Bar::new(vec![k.clone()], vec![v])
                    .name("")
                    .x_axis(format!("x{}", idx + 1))
                    .y_axis(format!("y{}", idx + 1))
                    .text(format!("{:.2}%", v))
                    .hover_info(HoverInfo::None)
                    .marker(if k.eq("Unknown") {
                        Marker::new().color(NamedColor::Gray)
                    } else {
                        Marker::new()
                    });
                if let Some(total) = total {
                    trace = trace.hover_info(HoverInfo::Text).hover_text(format!(
                        "{:.0} {}",
                        v * total / 100.,
                        conf.currency
                    ));
                }
                plot.add_trace(trace);
            }
        }
    }
    plot.set_layout(layout);
    let output_file = if !conf.output_folder.is_empty() {
        format!(
            "{}/{}",
            conf.output_folder,
            conf.output_file_name.to_string_lossy()
        )
    } else {
        conf.output_file_name.to_string_lossy().to_string()
    };
    plot.write_html(format!("{}.html", output_file));
    if conf.image {
        plot.write_image(
            format!(
                "{}.{}",
                output_file,
                <ImageFormat as Into<PlotlyImageFormat>>::into(conf.image_format)
            ),
            conf.image_format.into(),
            1920,
            1080,
            conf.image_scale,
        );
    }
    if conf.display {
        plot.show();
    }
    Ok(())
}
