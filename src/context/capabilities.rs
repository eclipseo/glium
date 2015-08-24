use context::ExtensionsList;
use version::Version;
use version::Api;
use std::cmp;
use std::ffi::CStr;
use std::mem;
use gl;

/// Represents the capabilities of the context.
///
/// Contrary to the state, these values never change.
pub struct Capabilities {
    /// List of versions of GLSL that are supported by the compiler.
    ///
    /// An empty list means that the backend doesn't have a compiler.
    pub supported_glsl_versions: Vec<Version>,

    /// True if out-of-bound access on the GPU side can't result in crashes.
    pub robustness: bool,

    /// True if it is possible for the OpenGL context to be lost.
    pub can_lose_context: bool,

    /// What happens when you change the current OpenGL context.
    pub release_behavior: ReleaseBehavior,

    /// Whether the context supports left and right buffers.
    pub stereo: bool,

    /// True if the default framebuffer is in sRGB.
    pub srgb: bool,

    /// Number of bits in the default framebuffer's depth buffer
    pub depth_bits: Option<u16>,

    /// Number of bits in the default framebuffer's stencil buffer
    pub stencil_bits: Option<u16>,

    /// Maximum number of textures that can be bound to a program.
    ///
    /// `glActiveTexture` must be between `GL_TEXTURE0` and `GL_TEXTURE0` + this value - 1.
    pub max_combined_texture_image_units: gl::types::GLint,

    /// Maximum value for `GL_TEXTURE_MAX_ANISOTROPY_EXT​`.
    ///
    /// `None` if the extension is not supported by the hardware.
    pub max_texture_max_anisotropy: Option<gl::types::GLfloat>,

    /// Maximum size of a buffer texture. `None` if this is not supported.
    pub max_texture_buffer_size: Option<gl::types::GLint>,

    /// Maximum width and height of `glViewport`.
    pub max_viewport_dims: (gl::types::GLint, gl::types::GLint),

    /// Maximum number of elements that can be passed with `glDrawBuffers`.
    pub max_draw_buffers: gl::types::GLint,

    /// Maximum number of vertices per patch. `None` if tessellation is not supported.
    pub max_patch_vertices: Option<gl::types::GLint>,

    /// Number of available buffer bind points for `GL_ATOMIC_COUNTER_BUFFER`.
    pub max_indexed_atomic_counter_buffer: gl::types::GLint,

    /// Number of available buffer bind points for `GL_SHADER_STORAGE_BUFFER`.
    pub max_indexed_shader_storage_buffer: gl::types::GLint,

    /// Number of available buffer bind points for `GL_TRANSFORM_FEEDBACK_BUFFER`.
    pub max_indexed_transform_feedback_buffer: gl::types::GLint,

    /// Number of available buffer bind points for `GL_UNIFORM_BUFFER`.
    pub max_indexed_uniform_buffer: gl::types::GLint,

    /// Number of work groups for compute shaders.
    pub max_compute_work_group_count: (gl::types::GLint, gl::types::GLint, gl::types::GLint),
}

/// Defines what happens when you change the current context.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReleaseBehavior {
    /// Nothing is done when using another context.
    None,

    /// The commands queue of the current context is flushed.
    Flush,
}

/// Loads the capabilities.
///
/// *Safety*: the OpenGL context corresponding to `gl` must be current in the thread.
///
/// ## Panic
///
/// Can panic if the version number or extensions list don't match the backend, leading to
/// unloaded functions being called.
///
pub unsafe fn get_capabilities(gl: &gl::Gl, version: &Version, extensions: &ExtensionsList)
                               -> Capabilities
{
    // getting the value of `GL_RENDERER`
    let renderer = unsafe {
        let s = gl.GetString(gl::RENDERER);
        assert!(!s.is_null());
        String::from_utf8(CStr::from_ptr(s as *const i8).to_bytes().to_vec()).ok()
                                    .expect("glGetString(GL_RENDERER) returned an non-UTF8 string")
    };

    Capabilities {
        supported_glsl_versions: {
            get_supported_glsl(gl, version, extensions)
        },

        robustness: if version >= &Version(Api::Gl, 4, 5) || version >= &Version(Api::GlEs, 3, 2) ||
                       (version >= &Version(Api::Gl, 3, 0) && extensions.gl_arb_robustness)
        {
            // TODO: there seems to be no way to query `GL_CONTEXT_FLAGS` before OpenGL 3.0, even
            //       if `GL_ARB_robustness` is there
            let mut val = mem::uninitialized();
            gl.GetIntegerv(gl::CONTEXT_FLAGS, &mut val);
            let val = val as gl::types::GLenum;
            (val & gl::CONTEXT_FLAG_ROBUST_ACCESS_BIT) != 0

        } else if extensions.gl_khr_robustness || extensions.gl_ext_robustness {
            let mut val = mem::uninitialized();
            gl.GetBooleanv(gl::CONTEXT_ROBUST_ACCESS, &mut val);
            val != 0

        } else {
            false
        },

        can_lose_context: if version >= &Version(Api::Gl, 4, 5) || extensions.gl_khr_robustness ||
                             extensions.gl_arb_robustness || extensions.gl_ext_robustness
        {
            let mut val = mem::uninitialized();
            gl.GetIntegerv(gl::RESET_NOTIFICATION_STRATEGY, &mut val);

            match val as gl::types::GLenum {
                gl::LOSE_CONTEXT_ON_RESET => true,
                gl::NO_RESET_NOTIFICATION => false,

                // WORK-AROUND: AMD drivers erroneously return this value, which doesn't even
                //              correspond to any GLenum in the specs. We work around this bug
                //              by interpreting it as `false`.
                0x31BE => false,

                _ => unreachable!()
            }

        } else {
            false
        },

        release_behavior: if extensions.gl_khr_context_flush_control {
            let mut val = mem::uninitialized();
            gl.GetIntegerv(gl::CONTEXT_RELEASE_BEHAVIOR, &mut val);

            match val as gl::types::GLenum {
                gl::NONE => ReleaseBehavior::None,
                gl::CONTEXT_RELEASE_BEHAVIOR_FLUSH => ReleaseBehavior::Flush,
                _ => unreachable!()
            }

        } else {
            ReleaseBehavior::Flush
        },

        stereo: {
            if version >= &Version(Api::Gl, 1, 0) {
                let mut val: gl::types::GLboolean = mem::uninitialized();
                gl.GetBooleanv(gl::STEREO, &mut val);
                val != 0
            } else {
                false
            }
        },

        srgb: {
            // `glGetFramebufferAttachmentParameteriv` incorrectly returns GL_INVALID_ENUM on some
            // drivers, so we prefer using `glGetIntegerv` if possible.
            if version >= &Version(Api::Gl, 3, 0) && !extensions.gl_ext_framebuffer_srgb {
                let mut value = mem::uninitialized();
                gl.GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER, gl::BACK_LEFT,
                                                       gl::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING,
                                                       &mut value);
                value as gl::types::GLenum == gl::SRGB

            } else if extensions.gl_ext_framebuffer_srgb {
                let mut value = mem::uninitialized();
                gl.GetBooleanv(gl::FRAMEBUFFER_SRGB_CAPABLE_EXT, &mut value);
                value != 0

            } else {
                false
            }
        },

        depth_bits: {
            let mut value = mem::uninitialized();

            // `glGetFramebufferAttachmentParameteriv` incorrectly returns GL_INVALID_ENUM on some
            // drivers, so we prefer using `glGetIntegerv` if possible.
            //
            // Also note that `gl_arb_es2_compatibility` may provide `GL_DEPTH_BITS` but os/x
            // doesn't even though it provides this extension. I'm not sure whether this is a bug
            // with OS/X or just the extension actually not providing it.
            if version >= &Version(Api::Gl, 3, 0) && !extensions.gl_arb_compatibility {
                let mut ty = mem::uninitialized();
                gl.GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER, gl::DEPTH,
                                                       gl::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
                                                       &mut ty);

                if ty as gl::types::GLenum == gl::NONE {
                    value = 0;
                } else {
                    gl.GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER, gl::DEPTH,
                                                           gl::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE,
                                                           &mut value);
                }

            } else {
                gl.GetIntegerv(gl::DEPTH_BITS, &mut value);
            };

            match value {
                0 => None,
                v => Some(v as u16),
            }
        },

        stencil_bits: {
            let mut value = mem::uninitialized();

            // `glGetFramebufferAttachmentParameteriv` incorrectly returns GL_INVALID_ENUM on some
            // drivers, so we prefer using `glGetIntegerv` if possible.
            //
            // Also note that `gl_arb_es2_compatibility` may provide `GL_STENCIL_BITS` but os/x
            // doesn't even though it provides this extension. I'm not sure whether this is a bug
            // with OS/X or just the extension actually not providing it.
            if version >= &Version(Api::Gl, 3, 0) && !extensions.gl_arb_compatibility {
                let mut ty = mem::uninitialized();
                gl.GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER, gl::STENCIL,
                                                       gl::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
                                                       &mut ty);

                if ty as gl::types::GLenum == gl::NONE {
                    value = 0;
                } else {
                    gl.GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER, gl::STENCIL,
                                                           gl::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE,
                                                           &mut value);
                }

            } else {
                gl.GetIntegerv(gl::STENCIL_BITS, &mut value);
            };

            match value {
                0 => None,
                v => Some(v as u16),
            }
        },

        max_combined_texture_image_units: {
            let mut val = 2;
            gl.GetIntegerv(gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS, &mut val);

            // WORK-AROUND (issue #1181)
            // Some Radeon drivers crash if you use texture units 32 or more.
            if renderer.contains("Radeon") {
                val = cmp::min(val, 32);
            }

            val
        },

        max_texture_max_anisotropy: if !extensions.gl_ext_texture_filter_anisotropic {
            None

        } else {
            Some({
                let mut val = mem::uninitialized();
                gl.GetFloatv(gl::MAX_TEXTURE_MAX_ANISOTROPY_EXT, &mut val);
                val
            })
        },

        max_texture_buffer_size: {
            if version >= &Version(Api::Gl, 3, 0) || extensions.gl_arb_texture_buffer_object ||
               extensions.gl_ext_texture_buffer_object || extensions.gl_oes_texture_buffer ||
               extensions.gl_ext_texture_buffer
            {
                Some({
                    let mut val = mem::uninitialized();
                    gl.GetIntegerv(gl::MAX_TEXTURE_BUFFER_SIZE, &mut val);
                    val
                })

            } else {
                None
            }
        },

        max_viewport_dims: {
            let mut val: [gl::types::GLint; 2] = [ 0, 0 ];
            gl.GetIntegerv(gl::MAX_VIEWPORT_DIMS, val.as_mut_ptr());
            (val[0], val[1])
        },

        max_draw_buffers: {
            if version >= &Version(Api::Gl, 2, 0) ||
                version >= &Version(Api::GlEs, 3, 0) ||
                extensions.gl_ati_draw_buffers || extensions.gl_arb_draw_buffers
            {
                let mut val = 1;
                gl.GetIntegerv(gl::MAX_DRAW_BUFFERS, &mut val);
                val
            } else {
                1
            }
        },

        max_patch_vertices: if version >= &Version(Api::Gl, 4, 0) ||
            extensions.gl_arb_tessellation_shader
        {
            Some({
                let mut val = mem::uninitialized();
                gl.GetIntegerv(gl::MAX_PATCH_VERTICES, &mut val);
                val
            })

        } else {
            None
        },

        max_indexed_atomic_counter_buffer: if version >= &Version(Api::Gl, 4, 2) {      // TODO: ARB_shader_atomic_counters   // TODO: GLES
            let mut val = mem::uninitialized();
            gl.GetIntegerv(gl::MAX_ATOMIC_COUNTER_BUFFER_BINDINGS, &mut val);
            val
        } else {
            0
        },

        max_indexed_shader_storage_buffer: {
            if version >= &Version(Api::Gl, 4, 3) || extensions.gl_arb_shader_storage_buffer_object {      // TODO: GLES
                let mut val = mem::uninitialized();
                gl.GetIntegerv(gl::MAX_SHADER_STORAGE_BUFFER_BINDINGS, &mut val);
                val
            } else {
                0
            }
        },

        max_indexed_transform_feedback_buffer: {
            if version >= &Version(Api::Gl, 4, 0) || extensions.gl_arb_transform_feedback3 {      // TODO: GLES
                let mut val = mem::uninitialized();
                gl.GetIntegerv(gl::MAX_TRANSFORM_FEEDBACK_BUFFERS, &mut val);
                val
            } else if version >= &Version(Api::Gl, 3, 0) || extensions.gl_ext_transform_feedback {
                let mut val = mem::uninitialized();
                gl.GetIntegerv(gl::MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS_EXT, &mut val);
                val
            } else {
                0
            }
        },

        max_indexed_uniform_buffer: {
            if version >= &Version(Api::Gl, 3, 1) || extensions.gl_arb_uniform_buffer_object {      // TODO: GLES
                let mut val = mem::uninitialized();
                gl.GetIntegerv(gl::MAX_UNIFORM_BUFFER_BINDINGS, &mut val);
                val
            } else {
                0
            }
        },

        max_compute_work_group_count: if version >= &Version(Api::Gl, 4, 3) ||
                                         version >= &Version(Api::GlEs, 3, 1) ||
                                         extensions.gl_arb_compute_shader
        {
            let mut val1 = mem::uninitialized();
            let mut val2 = mem::uninitialized();
            let mut val3 = mem::uninitialized();
            gl.GetIntegeri_v(gl::MAX_COMPUTE_WORK_GROUP_COUNT, 0, &mut val1);
            gl.GetIntegeri_v(gl::MAX_COMPUTE_WORK_GROUP_COUNT, 1, &mut val2);
            gl.GetIntegeri_v(gl::MAX_COMPUTE_WORK_GROUP_COUNT, 2, &mut val3);
            (val1, val2, val3)

        } else {
            (0, 0, 0)
        },
    }
}

/// Gets the list of GLSL versions supported by the backend.
///
/// *Safety*: the OpenGL context corresponding to `gl` must be current in the thread.
///
/// ## Panic
///
/// Can panic if the version number or extensions list don't match the backend, leading to
/// unloaded functions being called.
///
pub unsafe fn get_supported_glsl(gl: &gl::Gl, version: &Version, extensions: &ExtensionsList)
                                 -> Vec<Version>
{
    // checking if the implementation has a shader compiler
    // a compiler is optional in OpenGL ES
    if version.0 == Api::GlEs {
        let mut val = mem::uninitialized();
        gl.GetBooleanv(gl::SHADER_COMPILER, &mut val);
        if val == 0 {
            return vec![];
        }
    }

    // some recent versions have an API to determine the list of supported versions
    if version >= &Version(Api::Gl, 4, 3) {
        // FIXME: implement this and return the result directly
    }

    let mut result = Vec::with_capacity(8);

    if version >= &Version(Api::GlEs, 2, 0) || version >= &Version(Api::Gl, 4, 1) ||
       extensions.gl_arb_es2_compatibility
    {
        result.push(Version(Api::GlEs, 1, 0));
    }

    if version >= &Version(Api::GlEs, 3, 0) || version >= &Version(Api::Gl, 4, 3) ||
       extensions.gl_arb_es3_compatibility
    {
        result.push(Version(Api::GlEs, 3, 0));
    }

    if version >= &Version(Api::GlEs, 3, 1) || version >= &Version(Api::Gl, 4, 5) ||
       extensions.gl_arb_es3_1_compatibility
    {
        result.push(Version(Api::GlEs, 3, 1));
    }

    if version >= &Version(Api::GlEs, 3, 2) || extensions.gl_arb_es3_2_compatibility {
        result.push(Version(Api::GlEs, 3, 2));
    }

    if version >= &Version(Api::Gl, 2, 0) && version <= &Version(Api::Gl, 3, 0) ||
       extensions.gl_arb_compatibility
    {
        result.push(Version(Api::Gl, 1, 1));
    }

    if version >= &Version(Api::Gl, 2, 1) && version <= &Version(Api::Gl, 3, 0) ||
       extensions.gl_arb_compatibility
    {
        result.push(Version(Api::Gl, 1, 2));
    }

    if version == &Version(Api::Gl, 3, 0) || extensions.gl_arb_compatibility {
        result.push(Version(Api::Gl, 1, 3));
    }

    if version >= &Version(Api::Gl, 3, 1) {
        result.push(Version(Api::Gl, 1, 4));
    }

    if version >= &Version(Api::Gl, 3, 2) {
        result.push(Version(Api::Gl, 1, 5));
    }

    for &(major, minor) in &[(3, 3), (4, 0), (4, 1), (4, 2), (4, 3), (4, 4), (4, 5)] {
        if version >= &Version(Api::Gl, major, minor) {
            result.push(Version(Api::Gl, major, minor));
        }
    }

    result
}
