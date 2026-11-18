class_name WorldMeshes
extends RefCounted
## Converts the core's shared mesh-array contract into a Godot mesh for world surfaces.

## Normals further apart than this (degrees) are kept apart when simplifying, as Godot's
## importer does by default: creases stay creases.
const LOD_NORMAL_MERGE_ANGLE: float = 60.0


## A `compressed` mesh takes 20 bytes of video memory a vertex instead of 28: Godot
## keeps its positions in 16 bits across the mesh's bounds and its UVs in 16 bits across their
## largest value. Across a 480 m chunk that is millimetres, so neighbouring chunks still meet, but
## UVs in metres along a way kilometres long step by decimetres, too coarse for markings: only
## surfaces whose UVs stay short or go unused ask for it.
static func from_arrays(arrays: Dictionary, compressed: bool = false) -> ArrayMesh:
	var surface: Array = _surface(arrays)
	var mesh: ArrayMesh = ArrayMesh.new()
	var vertices: PackedVector3Array = surface[Mesh.ARRAY_VERTEX]
	if not vertices.is_empty():
		mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, surface, [], {}, _flags(compressed))
	return mesh


## The mesh of `arrays` with coarser versions for the distance, which Godot picks by their size
## on screen so that they stay within about a pixel of the full mesh. Their edges stay where they
## are: neighbouring chunks still meet. Slow, and touching no server: made on a worker thread,
## turned into a mesh with `get_mesh()` on the main one. `compressed` as in `from_arrays`.
static func simplified(arrays: Dictionary, compressed: bool = false) -> ImporterMesh:
	var importer: ImporterMesh = ImporterMesh.new()
	var surface: Array = _surface(arrays)
	var vertices: PackedVector3Array = surface[Mesh.ARRAY_VERTEX]
	if not vertices.is_empty():
		importer.add_surface(
			Mesh.PRIMITIVE_TRIANGLES, surface, [], {}, null, "", _flags(compressed)
		)
		importer.generate_lods(LOD_NORMAL_MERGE_ANGLE, 0.0, [])
	return importer


static func _flags(compressed: bool) -> int:
	return Mesh.ARRAY_FLAG_COMPRESS_ATTRIBUTES if compressed else 0


static func _surface(arrays: Dictionary) -> Array:
	var surface: Array = []
	surface.resize(Mesh.ARRAY_MAX)
	var vertices: PackedVector3Array = arrays["vertices"]
	var normals: PackedVector3Array = arrays["normals"]
	var uvs: PackedVector2Array = arrays["uvs"]
	var colors: PackedColorArray = arrays["colors"]
	var indices: PackedInt32Array = arrays["indices"]
	surface[Mesh.ARRAY_VERTEX] = vertices
	surface[Mesh.ARRAY_NORMAL] = normals
	surface[Mesh.ARRAY_TEX_UV] = uvs
	if arrays.has("uv2"):
		var uv2: PackedVector2Array = arrays["uv2"]
		surface[Mesh.ARRAY_TEX_UV2] = uv2
	if not colors.is_empty():
		surface[Mesh.ARRAY_COLOR] = colors
	surface[Mesh.ARRAY_INDEX] = indices
	return surface
