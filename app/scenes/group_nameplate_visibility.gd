class_name GroupNameplateVisibility
extends CompositorEffect
## Eight depth samples let native UI badges respect scenery without a second world render.

const SLOTS: int = 8

var _mask: VisibilityTexture = VisibilityTexture.new()
var _available: bool = false
var _hidden: ImageTexture
var _rd: RenderingDevice
var _shader: RID
var _pipeline: RID
var _sampler: RID
var _texture: RID
var _samples: PackedByteArray = PackedByteArray()


## Canvas materials can retain the last mask after the camera's compositor is detached.
class VisibilityTexture:
	extends Texture2DRD
	var device: RenderingDevice

	func _notification(what: int) -> void:
		if what == NOTIFICATION_PREDELETE and device != null and texture_rd_rid.is_valid():
			RenderingServer.call_on_render_thread(device.free_rid.bind(texture_rd_rid))


func _init() -> void:
	effect_callback_type = EFFECT_CALLBACK_TYPE_POST_TRANSPARENT
	access_resolved_depth = true
	_samples.resize(SLOTS * 16)
	var hidden: Image = Image.create(SLOTS, 1, false, Image.FORMAT_RGBA8)
	hidden.fill(Color.BLACK)
	_hidden = ImageTexture.create_from_image(hidden)


func update_samples(data: PackedFloat32Array) -> void:
	RenderingServer.call_on_render_thread(_set_samples.bind(data.to_byte_array()))


## A valid initial texture keeps badges hidden until their first visibility result is ready.
func visibility_texture() -> Texture2D:
	return _mask if _available else _hidden


## The camera API uses OpenGL depth; the renderer's depth texture uses reversed Z.
static func sample(camera: Camera3D, screen_point: Vector2, world_point: Vector3) -> Vector4:
	var view: Vector3 = camera.global_transform.affine_inverse() * world_point
	var clip: Vector4 = camera.get_camera_projection() * Vector4(view.x, view.y, view.z, 1.0)
	var uv: Vector2 = screen_point / camera.get_viewport().get_visible_rect().size
	return Vector4(uv.x, uv.y, (1.0 - clip.z / clip.w) * 0.5, 1.0)


func _set_samples(data: PackedByteArray) -> void:
	_samples = data


func _initialize_gpu() -> void:
	_rd = RenderingServer.get_rendering_device()
	_sampler = _rd.sampler_create(RDSamplerState.new())
	var format: RDTextureFormat = RDTextureFormat.new()
	format.width = SLOTS
	format.height = 1
	format.format = RenderingDevice.DATA_FORMAT_R8G8B8A8_UNORM
	format.usage_bits = (
		RenderingDevice.TEXTURE_USAGE_STORAGE_BIT | RenderingDevice.TEXTURE_USAGE_SAMPLING_BIT
	)
	_texture = _rd.texture_create(format, RDTextureView.new())
	_mask.device = _rd
	_mask.texture_rd_rid = _texture
	_mark_available.call_deferred()


func _prepare_shader() -> void:
	var source: RDShaderSource = RDShaderSource.new()
	source.source_compute = """#version 450
layout(local_size_x = 8) in;
layout(set = 0, binding = 0) uniform sampler2D scene_depth;
layout(rgba8, set = 0, binding = 1) uniform writeonly image2D visible;
layout(push_constant, std430) uniform Points { vec4 points[8]; } params;
void main() {
    uint index = gl_GlobalInvocationID.x;
    vec4 point = params.points[index];
    bool clear = point.w > 0.0 && textureLod(scene_depth, point.xy, 0.0).r <= point.z + 0.00001;
    imageStore(visible, ivec2(int(index), 0), vec4(vec3(clear ? 1.0 : 0.0), 1.0));
}
"""
	_shader = _rd.shader_create_from_spirv(_rd.shader_compile_spirv_from_source(source))
	_pipeline = _rd.compute_pipeline_create(_shader)


func _mark_available() -> void:
	_available = true


func _render_callback(_type: int, data: RenderData) -> void:
	var buffers: RenderSceneBuffersRD = data.get_render_scene_buffers() as RenderSceneBuffersRD
	if buffers == null:
		return
	if _rd == null:
		_initialize_gpu()
		_prepare_shader()
	var input: RDUniform = RDUniform.new()
	input.uniform_type = RenderingDevice.UNIFORM_TYPE_SAMPLER_WITH_TEXTURE
	input.binding = 0
	input.add_id(_sampler)
	input.add_id(buffers.get_depth_texture())
	var output: RDUniform = RDUniform.new()
	output.uniform_type = RenderingDevice.UNIFORM_TYPE_IMAGE
	output.binding = 1
	output.add_id(_texture)
	var uniforms: RID = UniformSetCacheRD.get_cache(_shader, 0, [input, output])
	var list: int = _rd.compute_list_begin()
	_rd.compute_list_bind_compute_pipeline(list, _pipeline)
	_rd.compute_list_bind_uniform_set(list, uniforms, 0)
	_rd.compute_list_set_push_constant(list, _samples, 128)
	_rd.compute_list_dispatch(list, 1, 1, 1)
	_rd.compute_list_end()


func _notification(what: int) -> void:
	if what == NOTIFICATION_PREDELETE and _rd != null:
		for resource: RID in [_shader, _sampler]:
			if resource.is_valid():
				RenderingServer.call_on_render_thread(_rd.free_rid.bind(resource))
