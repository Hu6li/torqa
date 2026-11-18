extends "res://tools/render_models.gd"
## Production model selection, shell materials and Rust-derived façades at fixed cameras.


func _run() -> void:
	_stage()
	_row.hide()
	_out = OS.get_environment("OUT_DIR")
	if _out.is_empty():
		_out = ProjectSettings.globalize_path("res://../screenshots/building-integration")
	DirAccess.make_dir_recursive_absolute(_out)
	var source: String = ProjectSettings.globalize_path(
		"res://../screenshots/building-integration/fixture.json"
	)
	var fixture: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(source))
	var material: ShaderMaterial = ShaderMaterial.new()
	BuildingModels.configure_shell(material)
	# The fade kinds stand inside the band where the close and ordinary appearances swap.
	for kind: String in ["model", "facade", "model-far", "facade-far", "model-150", "model-170"]:
		for preset: String in ["medium", "ultra"]:
			var stage: Node3D = Node3D.new()
			root.add_child(stage)
			if kind.begins_with("model"):
				var walls: Color = Palette.colors("buildings.walls")[0]
				var roof: Color = Palette.colors("buildings.tiles")[1]
				var buffer: PackedFloat32Array = [
					1,
					0,
					0,
					0,
					0,
					1,
					0,
					0,
					0,
					0,
					1,
					0,
					walls.r,
					walls.g,
					walls.b,
					1,
					roof.r,
					roof.g,
					roof.b,
					0.5
				]
				var cell: Dictionary = {
					"models": {"house_gable_2_m": buffer}, "shells": _arrays({})
				}
				BuildingModels.add_to(stage, cell, material, 400, 1800, preset == "ultra")
				_camera.look_at_from_position(Vector3(13, 1.75, 20), Vector3(0, 4.1, 0))
			else:
				var shell: MeshInstance3D = MeshInstance3D.new()
				var shell_data: Dictionary = fixture["shell"]
				shell.mesh = WorldMeshes.from_arrays(_arrays(shell_data))
				shell.material_override = material
				stage.add_child(shell)
				if preset == "ultra":
					var details: Array = []
					for entry: Dictionary in fixture["details"]:
						details.append(_arrays(entry))
					BuildingDetails.add_to(stage, details)
				_camera.look_at_from_position(Vector3(19, 1.75, 26), Vector3(0, 6, 0))
			if kind.ends_with("far"):
				_camera.look_at_from_position(Vector3(104, 1.75, 160), Vector3(0, 5, 0))
			elif kind.begins_with("model-"):
				var away: Vector2 = Vector2(13, 20).normalized() * float(kind.get_slice("-", 1))
				_camera.look_at_from_position(Vector3(away.x, 1.75, away.y), Vector3(0, 4.1, 0))
				_camera.fov = 6.0
			for frame: int in range(8):
				await process_frame
			await RenderingServer.frame_post_draw
			var picture: Image = root.get_texture().get_image()
			picture.save_png(_out.path_join(kind + "-" + preset + ".png"))
			print("BUILDING saved ", kind, " ", preset)
			stage.queue_free()
			_camera.fov = 40.0
			await process_frame
	quit(0)


func _arrays(data: Dictionary) -> Dictionary:
	var vertices: PackedVector3Array = PackedVector3Array()
	var normals: PackedVector3Array = PackedVector3Array()
	var uvs: PackedVector2Array = PackedVector2Array()
	var colors: PackedColorArray = PackedColorArray()
	for value: Array in data.get("vertices", []):
		vertices.append(_vector(value))
	for value: Array in data.get("normals", []):
		normals.append(_vector(value))
	for value: Array in data.get("uvs", []):
		var x: float = value[0]
		var y: float = value[1]
		uvs.append(Vector2(x, y))
	for value: Array in data.get("colors", []):
		var r: float = value[0]
		var g: float = value[1]
		var b: float = value[2]
		var a: float = value[3]
		colors.append(Color(r, g, b, a))
	var indices: Array = data.get("indices", [])
	return {
		"vertices": vertices,
		"normals": normals,
		"uvs": uvs,
		"colors": colors,
		"indices": PackedInt32Array(indices)
	}


func _vector(value: Array) -> Vector3:
	var x: float = value[0]
	var y: float = value[1]
	var z: float = value[2]
	return Vector3(x, y, z)
