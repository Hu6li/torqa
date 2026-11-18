class_name BuildingDetailStreaming
extends RefCounted
## Keep only the nearby façade additions in memory, including a margin for moving cameras.
## Additions come in 64 m cells: moving on adds the cells newly in reach and lets go of those
## left behind, while the cells still in reach stay uploaded. The core reads each chunk's
## façades once while the camera is near it.

const STEP: float = 64.0
const LOAD_RANGE: float = BuildingDetails.RANGE + BuildingDetails.FADE + STEP * 2.0

var _entries: Array[Dictionary] = []
## The core's let-go call from the last update, for chunks forgotten between updates.
var _let_go: Callable = Callable()


func reset() -> void:
	for entry: Dictionary in _entries:
		_release(entry)
	_entries.clear()


func register(index: int, node: Node3D, bounds: AABB) -> void:
	unregister(index)
	var corner: Vector3 = node.position + bounds.position
	var cells: Dictionary[Vector2i, Node3D] = {}
	_entries.append(
		{
			"index": index,
			"parent": node,
			"bounds": Rect2(Vector2(corner.x, corner.z), Vector2(bounds.size.x, bounds.size.z)),
			"focus": Vector2(INF, INF),
			"cells": cells
		}
	)


## Forgets a chunk the streaming let go of; its additions go with the chunk's node.
func unregister(index: int) -> void:
	for at: int in range(_entries.size() - 1, -1, -1):
		var entry: Dictionary = _entries[at]
		if entry["index"] == index:
			_release(entry)
			_entries.remove_at(at)


## `load_details(index, x, z, radius, loaded)` returns the cells `added` and `dropped` around
## world `(x, z)`; `let_go(index)` lets the core forget a chunk's façades.
func update(focus: Vector2, load_details: Callable, let_go: Callable) -> void:
	_let_go = let_go
	var built: bool = false
	for entry: Dictionary in _entries:
		var bounds: Rect2 = entry["bounds"]
		var previous: Vector2 = entry["focus"]
		if not bounds.grow(LOAD_RANGE).has_point(focus):
			if previous.is_finite():
				_release(entry)
		elif not built and previous.distance_to(focus) >= STEP:
			_refresh(entry, focus, load_details)
			built = true


func ready_at(focus: Vector2) -> bool:
	for entry: Dictionary in _entries:
		var bounds: Rect2 = entry["bounds"]
		var previous: Vector2 = entry["focus"]
		if bounds.grow(LOAD_RANGE).has_point(focus) and previous.distance_to(focus) >= STEP:
			return false
	return true


func _refresh(entry: Dictionary, focus: Vector2, load_details: Callable) -> void:
	var cells: Dictionary[Vector2i, Node3D] = entry["cells"]
	var loaded: PackedInt32Array = PackedInt32Array()
	for cell: Vector2i in cells:
		loaded.append(cell.x)
		loaded.append(cell.y)
	var index: int = entry["index"]
	var change: Dictionary = load_details.call(index, focus.x, focus.y, LOAD_RANGE, loaded)
	var dropped: Array = change.get("dropped", [])
	for cell: Vector2i in dropped:
		var node: Node3D = cells.get(cell)
		cells.erase(cell)
		if node != null and is_instance_valid(node):
			node.queue_free()
	var added: Dictionary = change.get("added", {})
	var parent: Node3D = entry["parent"]
	for cell: Vector2i in added:
		var node: Node3D = Node3D.new()
		parent.add_child(node)
		BuildingDetails.add_to(node, [added[cell]])
		cells[cell] = node
	entry["focus"] = focus


func _release(entry: Dictionary) -> void:
	var cells: Dictionary[Vector2i, Node3D] = entry["cells"]
	for cell: Vector2i in cells:
		var node: Node3D = cells[cell]
		if node != null and is_instance_valid(node):
			node.queue_free()
	cells.clear()
	entry["focus"] = Vector2(INF, INF)
	if _let_go.is_valid():
		var index: int = entry["index"]
		_let_go.call(index)
