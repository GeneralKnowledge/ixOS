//! Air quality index bands, from the official band tables:
//!
//! * European Air Quality Index, as Open-Meteo documents it (European
//!   Environment Agency bands): 0–20 good, 20–40 fair, 40–60 moderate,
//!   60–80 poor, 80–100 very poor, over 100 extremely poor.
//! * US AQI (US EPA): 0–50 good, 51–100 moderate, 101–150 unhealthy for
//!   sensitive groups, 151–200 unhealthy, 201–300 very unhealthy, 301 and
//!   over hazardous.

use super::types::{AirQualityHour, AirQualityScale, AqiReading};

/// `value` on `scale`, placed in its band.
pub(crate) fn aqi_reading(value: u16, scale: AirQualityScale) -> AqiReading {
    let (level, label) = match scale {
        AirQualityScale::European => match value {
            0..20 => (1, "Good"),
            20..40 => (2, "Fair"),
            40..60 => (3, "Moderate"),
            60..80 => (4, "Poor"),
            80..=100 => (5, "Very poor"),
            _ => (6, "Extremely poor"),
        },
        AirQualityScale::UnitedStates => match value {
            0..=50 => (1, "Good"),
            51..=100 => (2, "Moderate"),
            101..=150 => (3, "Unhealthy for sensitive groups"),
            151..=200 => (4, "Unhealthy"),
            201..=300 => (5, "Very unhealthy"),
            _ => (6, "Hazardous"),
        },
    };
    AqiReading {
        value,
        scale,
        level,
        label: label.into(),
    }
}

/// The hour's index on `scale`, if the source has one.
pub(crate) fn hour_reading(hour: &AirQualityHour, scale: AirQualityScale) -> Option<AqiReading> {
    let value = match scale {
        AirQualityScale::European => hour.european_aqi,
        AirQualityScale::UnitedStates => hour.us_aqi,
    }?;
    Some(aqi_reading(value, scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn european_band_edges() {
        let eu = |v| aqi_reading(v, AirQualityScale::European);
        assert_eq!(eu(0).label, "Good");
        assert_eq!(eu(19).level, 1);
        assert_eq!(eu(20).label, "Fair");
        assert_eq!(eu(40).label, "Moderate");
        assert_eq!(eu(60).label, "Poor");
        assert_eq!(eu(80).label, "Very poor");
        assert_eq!(eu(100).level, 5);
        assert_eq!(eu(101).label, "Extremely poor");
    }

    #[test]
    fn us_band_edges() {
        let us = |v| aqi_reading(v, AirQualityScale::UnitedStates);
        assert_eq!(us(50).label, "Good");
        assert_eq!(us(51).label, "Moderate");
        assert_eq!(us(101).label, "Unhealthy for sensitive groups");
        assert_eq!(us(151).label, "Unhealthy");
        assert_eq!(us(201).label, "Very unhealthy");
        assert_eq!(us(301).label, "Hazardous");
        assert_eq!(us(500).level, 6);
    }
}
