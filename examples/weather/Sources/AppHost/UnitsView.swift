import SwiftUI
import ForgeUI

/// The person's display units. Changing one applies to every page at once
/// and is saved for the next launch.
struct UnitsView: View {
    @Bindable var model: WeatherViewModel

    var body: some View {
        Form {
            Section {
                picker("Temperature", \.temperature, id: "units.temperature",
                       [(.celsius, "Celsius (°C)"), (.fahrenheit, "Fahrenheit (°F)")])
                picker("Wind", \.wind, id: "units.wind",
                       [(.kilometresPerHour, "km/h"), (.milesPerHour, "mph"), (.metresPerSecond, "m/s"), (.knots, "Knots")])
                picker("Precipitation", \.precipitation, id: "units.precipitation",
                       [(.millimetres, "Millimetres"), (.inches, "Inches")])
                picker("Pressure", \.pressure, id: "units.pressure",
                       [(.hectopascals, "hPa"), (.inchesOfMercury, "inHg")])
                picker("Distance", \.distance, id: "units.distance",
                       [(.kilometres, "Kilometres"), (.miles, "Miles")])
            } footer: {
                if let error = model.unitsError {
                    Text(error).forgeForeground(.danger)
                }
            }
            Section {
                picker("Air quality index", \.airQuality, id: "units.airQuality",
                       [(.european, "European"), (.unitedStates, "US")])
            }
        }
        .navigationTitle("Units")
    }

    private func picker<Value: Hashable>(
        _ title: String, _ path: WritableKeyPath<UnitSettings, Value>, id: String, _ options: [(Value, String)]
    ) -> some View {
        Picker(title, selection: Binding(
            get: { model.units[keyPath: path] },
            set: { value in
                var units = model.units
                units[keyPath: path] = value
                model.setUnits(units)
            }
        )) {
            ForEach(options.indices, id: \.self) { index in
                Text(options[index].1).tag(options[index].0)
            }
        }
        .accessibilityIdentifier(id)
    }
}
