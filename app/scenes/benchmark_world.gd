class_name BenchmarkWorld
extends RideWorld
## Offline calibration using the ride's upload path, shaders and presets (R151).


func _process(delta: float) -> void:
	super._process(delta)
	_avatar.animate(delta, 90.0, 25.0)


## No rider data or course is bound to this world.
func show_benchmark(benchmark: OrqwaGraphicsBenchmark) -> void:
	_benchmark = benchmark
	_chunk_count = 1
	_road.mesh = WorldMeshes.from_arrays(benchmark.road())
	_road.material_override = _road_material
	_rider.position = Vector3(1.2, 0.02, 0)
	benchmark_camera(0.0)


## The same slow pan across village, water and woods for every measured preset.
func benchmark_camera(seconds: float) -> void:
	_camera.position = Vector3(1.2, 2.5, 7.0)
	_camera.look_at(Vector3(12.0 * sin(seconds * TAU / 3.0), 1.8, -25.0))
