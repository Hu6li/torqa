extends SceneTree
## Headless checks of the analysis across rides: the Analysis tab (R61), the power curve and a
## ride's power records on its summary (R62), the power profile (R63).
## Run: godot --headless --path app -s res://tests/analysis_smoke.gd

const MAIN_SCENE: String = "res://scenes/main.tscn"

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_power_curve_chart()
	_radar_chart()
	await _analysis_tab()
	if not _failed:
		print("ANALYSIS SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## The power curve (R62): durations named as riders say them, curves scaled to their data.
func _power_curve_chart() -> void:
	var labels: PackedStringArray = PackedStringArray()
	for seconds: float in [5.0, 60.0, 90.0, 1200.0, 3600.0, 5400.0]:
		labels.append(PowerCurveChart.duration_label(seconds))
	_expect(labels, ["5 s", "1 min", "1:30", "20 min", "1 h", "1:30:00"], "durations")
	var chart: PowerCurveChart = PowerCurveChart.new()
	chart.size = Vector2(600, 300)
	root.add_child(chart)
	chart.add_curve(PackedVector2Array(), Color.WHITE)
	var curves: Array = chart.get("_curves")
	_check(curves.is_empty(), "an empty curve is left out")
	chart.add_curve(PackedVector2Array([Vector2(1, 900), Vector2(7200, 210)]), Color.WHITE)
	var longest: float = chart.get("_longest")
	var highest: float = chart.get("_highest")
	_check(longest == 7200.0 and highest == 900.0, "scaled to it")
	var step: float = chart.call("_value_step")
	var top: float = chart.call("_top_value")
	_check(step == 250.0 and top == 1000.0, "round grid lines: %s to %s" % [step, top])
	# Efforts as the core gives them, in W/kg on request.
	chart.clear()
	var efforts: Array = [
		{"duration_s": 5.0, "power": 812.0, "watts_per_kg": 11.6, "start_unix_s": 0},
		{"duration_s": 300.0, "power": 312.4, "watts_per_kg": 4.46, "start_unix_s": 0},
	]
	chart.add_efforts(efforts, Color.WHITE, true)
	var added: Array = chart.get("_curves")
	var first: Dictionary = added[0]
	var points: PackedVector2Array = first["points"]
	_check(points[1] == Vector2(300, 4.46), "per kilogram on the curve: %s" % points)
	chart.free()


## The power profile (R63): a spoke per duration, rides missing on some of them.
func _radar_chart() -> void:
	var tab: AnalysisTab = AnalysisTab.new()
	root.add_child(tab)
	var axes: Array = [
		{"duration_s": 5.0, "period": 800.0, "best": 1000.0, "share": 0.8},
		{"duration_s": 60.0, "period": null, "best": 450.0, "share": null},
		{"duration_s": 300.0, "period": 350.0, "best": 350.0, "share": 1.0},
	]
	tab.call("_fill_radar", axes, false)
	var radar: RadarChart = tab.get("_radar")
	var spokes: Array = radar.get("_spokes")
	var shares: Array = spokes.map(func(spoke: Dictionary) -> float: return spoke["share"])
	_check(shares == [0.8, -1.0, 1.0], "a spoke without a ride of the period: %s" % [shares])
	var values: Array = spokes.map(func(spoke: Dictionary) -> String: return spoke["value"])
	_check(values == ["800 W", "--", "350 W"], "the period's values: %s" % [values])
	tab.free()


## The Analysis tab (R61) after History, its figures from the efforts the core gives (R62),
## and a ride's power records on its summary.
func _analysis_tab() -> void:
	var main: Control = (load(MAIN_SCENE) as PackedScene).instantiate()
	root.add_child(main)
	await process_frame
	var start: StartPage = main.get_node("StartPage")
	var tabs: TabContainer = start.get("_tabs")
	_check(tabs.get_tab_title(StartPage.Tab.ANALYSIS) == "Analysis", "an Analysis tab")
	tabs.current_tab = StartPage.Tab.ANALYSIS
	var analysis: AnalysisTab = start.get("_analysis")
	var empty: Label = analysis.get("_empty")
	var content: Control = analysis.get("_content")
	var scroll: ScrollContainer = analysis.get("_scroll")
	var period: Control = analysis.get("_period")
	_check(scroll.is_ancestor_of(period), "the heading and its choices scroll with the sections")
	_check(empty.visible != content.visible, "the curve or why there is none")
	var efforts: Array = [
		{"duration_s": 5.0, "power": 812.0, "watts_per_kg": 11.6, "start_unix_s": 0},
		{"duration_s": 300.0, "power": 312.4, "watts_per_kg": 4.46, "start_unix_s": 0},
	]
	var five_minutes: Dictionary = AnalysisTab._effort(efforts, 300)
	_check(AnalysisTab._value_text(five_minutes, false) == "312 W", "watts")
	_check(AnalysisTab._value_text(five_minutes, true) == "4.5 W/kg", "per kilogram")
	_check(AnalysisTab._value_text(AnalysisTab._effort(efforts, 60), false) == "--", "no effort")
	var history: HistoryScreen = main.get_node("HistoryScreen")
	var ride: Dictionary = {
		"power_records": PackedFloat64Array([300.0]),
		"power_curve": PackedVector2Array([Vector2(5, 700), Vector2(300, 312.4)]),
	}
	history.call("_fill_power_records", ride)
	var records: HFlowContainer = history.get("_power_records")
	var badges: Array[Node] = records.get_children()
	_check(
		badges.size() == 1 and (badges[0] as Label).text == "★ Best 5 min: 312 W",
		"a ★ per power record"
	)
	main.free()


func _expect(actual: PackedStringArray, expected: Array, what: String) -> void:
	_check(actual == PackedStringArray(expected), "%s: %s" % [what, actual])


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("ANALYSIS SMOKE TEST FAILED: " + what)
		_failed = true
