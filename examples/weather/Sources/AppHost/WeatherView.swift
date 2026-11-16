import SwiftUI
import ForgeKit
@preconcurrency import WeatherFFI

/// The whole app: ForgeKit opens the core (a spinner meanwhile, a recovery
/// screen if it can't), then the pager.
public struct WeatherRoot: View {
    @Bindable private var host: ForgeHost<ForgeWeatherApp>

    public init(host: ForgeHost<ForgeWeatherApp>) {
        self.host = host
    }

    public var body: some View {
        ForgeRoot(host) { core in
            WeatherView(model: WeatherViewModel(client: WeatherClient(core: core)))
        }
    }
}

/// A page per place ("My Location" first, then saved places) that you swipe
/// between, each drawn over its own sky, and a list of places for adding,
/// removing and reordering them.
public struct WeatherView: View {
    @State private var model: WeatherViewModel
    /// `-WeatherShowPlaces YES` opens the places list at launch (screenshots).
    @State private var showingLocations = UserDefaults.standard.bool(forKey: "WeatherShowPlaces")
    @Environment(\.scenePhase) private var scenePhase

    public init(model: WeatherViewModel) {
        _model = State(initialValue: model)
    }

    public var body: some View {
        pager
            // The pages' own skies stop at the safe area inside the pager;
            // this fills the status bar and home indicator strips with the
            // sky of the page on screen.
            .background {
                let overview = model.selectedPage?.overview
                SkyBackground(scene: overview?.scene ?? .cloudy, isDay: overview?.isDay ?? true, animated: false)
                    .ignoresSafeArea()
            }
            .safeAreaInset(edge: .bottom, spacing: 0) {
                BottomBar(model: model, showingLocations: $showingLocations)
            }
            // Pages sit on a drawn sky whatever the system appearance: white
            // text, and materials that read as frosted glass over it.
            .environment(\.colorScheme, .dark)
            .sheet(isPresented: $showingLocations) {
                LocationsView(model: model, isPresented: $showingLocations)
            }
            .task { await model.start() }
            .onChange(of: model.selection) { _, _ in model.pageDidAppear() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { model.becameActive() }
            }
    }

    @ViewBuilder private var pager: some View {
        #if os(iOS)
            TabView(selection: $model.selection) {
                ForEach(model.pages) { page in
                    ForecastPage(page: page, model: model, showLocations: { showingLocations = true })
                        .tag(page.id)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
        #else
            // `swift run` on a Mac: one page at a time, chosen from the bar.
            if let page = model.selectedPage {
                ForecastPage(page: page, model: model, showLocations: { showingLocations = true })
            }
        #endif
    }
}

/// Page dots (an arrow for "My Location"), refresh, and the places list.
private struct BottomBar: View {
    @Bindable var model: WeatherViewModel
    @Binding var showingLocations: Bool

    var body: some View {
        HStack(spacing: 12) {
            Group {
                if model.isRefreshing {
                    Button {
                        model.cancelRefresh()
                    } label: {
                        ProgressView().tint(.white)
                    }
                    .accessibilityLabel("Cancel refresh")
                    .accessibilityIdentifier("weather.cancelRefresh")
                } else {
                    Button {
                        model.refresh()
                    } label: {
                        Image(systemName: "arrow.clockwise")
                    }
                    .accessibilityLabel("Refresh")
                    .accessibilityIdentifier("weather.refresh")
                    .disabled(model.place == nil)
                }
            }
            .frame(width: 48, height: 48)
            .background(.ultraThinMaterial, in: Circle())

            Spacer(minLength: 0)
            PageDots(model: model)
            Spacer(minLength: 0)

            Button {
                showingLocations = true
            } label: {
                Image(systemName: "list.bullet")
            }
            .accessibilityLabel("Places")
            .accessibilityIdentifier("weather.places")
            .frame(width: 48, height: 48)
            .background(.ultraThinMaterial, in: Circle())
        }
        .font(.system(size: 19, weight: .medium))
        .foregroundStyle(.white)
        // The bar keeps its size at large text sizes, like the system's;
        // VoiceOver labels carry the meaning.
        .dynamicTypeSize(...DynamicTypeSize.xxLarge)
        .padding(.horizontal, 20)
        .padding(.top, 6)
        .padding(.bottom, 4)
    }
}

private struct PageDots: View {
    @Bindable var model: WeatherViewModel

    var body: some View {
        HStack(spacing: 9) {
            ForEach(model.pages) { page in
                let selected = page.id == model.selection
                Button {
                    withAnimation { model.selection = page.id }
                } label: {
                    Group {
                        if page.isMyLocation {
                            Image(systemName: "location.fill").font(.system(size: 10, weight: .bold))
                        } else {
                            Circle().frame(width: 7, height: 7)
                        }
                    }
                    .foregroundStyle(.white.opacity(selected ? 1 : 0.45))
                    .frame(minWidth: 12, minHeight: 24)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(page.place?.name ?? "My Location")
                .accessibilityAddTraits(selected ? .isSelected : [])
                .accessibilityIdentifier("weather.page.\(page.isMyLocation ? "here" : page.place?.name ?? "place")")
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 40)
        .background(.ultraThinMaterial, in: Capsule())
        .opacity(model.pages.count > 1 ? 1 : 0)
    }
}
