extends SceneTree
## Ride time follows real elapsed time even when Godot caps slow-frame delta (R233).

var _orqwa: OrqwaApp


func _initialize() -> void:
	_orqwa = OrqwaApp.new()
	root.add_child(_orqwa)
	_run.call_deferred()


func _run() -> void:
	assert(_orqwa.connect_fake_trainer(200.0, 85.0))
	assert(_orqwa.start_workout({"kind": "power", "power_w": 200.0}, false, false))
	var deadline_ms: int = Time.get_ticks_msec() + 10000
	while _orqwa.ride_state().get("power") == null:
		assert(Time.get_ticks_msec() < deadline_ms, "fake trainer telemetry timed out")
		await process_frame
	Engine.max_fps = 5
	await process_frame
	await process_frame
	var before: Dictionary = _orqwa.ride_state()
	var started_us: int = Time.get_ticks_usec()
	for _frame: int in range(10):
		await process_frame
	var after: Dictionary = _orqwa.ride_state()
	var wall_s: float = float(Time.get_ticks_usec() - started_us) / 1000000.0
	var ride_s: float = after["elapsed_s"] - before["elapsed_s"]
	assert(wall_s > 1.5, "the test must exercise slow real frames")
	assert(absf(ride_s - wall_s) < 0.1, "ride %.3fs vs real %.3fs" % [ride_s, wall_s])
	assert(after["distance_m"] > before["distance_m"], "the ride advances through slow frames")
	_orqwa.abort_ride()
	print("RIDE CLOCK SMOKE TEST PASSED")
	quit()
