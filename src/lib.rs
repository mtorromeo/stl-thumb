extern crate cgmath;
#[macro_use]
extern crate glium;
extern crate image;
extern crate libc;
#[macro_use]
extern crate log;
extern crate mint;

pub mod config;
mod mesh;

use libc::c_char;
use std::error::Error;
use std::ffi::CStr;
use std::fs::File;
use std::{io, thread, time, slice};
use config::Config;
//use cgmath::EuclideanSpace;
use glium::{glutin, Surface, CapabilitiesSource};
use mesh::Mesh;

#[cfg(target_os = "linux")]
use std::env;

// TODO: Move this stuff to config module
const CAM_FOV_DEG: f32 = 30.0;
//const CAM_POSITION: cgmath::Point3<f32> = cgmath::Point3 {x: 2.0, y: -4.0, z: 2.0};


fn print_matrix(m: [[f32; 4]; 4]) {
    for i in 0..4 {
        debug!("{:.3}\t{:.3}\t{:.3}\t{:.3}", m[i][0], m[i][1], m[i][2], m[i][3]);
    }
    debug!("");
}


fn print_context_info(display: &glium::backend::Context) 
{
    // Print context information
    info!("GL Version:   {:?}", display.get_opengl_version());
    info!("GL Version:   {}", display.get_opengl_version_string());
    info!("GLSL Version: {:?}", display.get_supported_glsl_version());
    info!("Vendor:       {}", display.get_opengl_vendor_string());
    info!("Renderer      {}", display.get_opengl_renderer_string());
    info!("Free GPU Mem: {:?}", display.get_free_video_memory());
    info!("Depth Bits:   {:?}\n", display.get_capabilities().depth_bits);
}


fn create_normal_display(config: &Config) -> Result<(glium::Display, glutin::EventsLoop), Box<dyn Error>> {
    let events_loop = glutin::EventsLoop::new();
    let window_dim = glutin::dpi::LogicalSize::new(
        config.width.into(),
        config.height.into());
    let window = glutin::WindowBuilder::new()
        .with_title("stl-thumb")
        .with_dimensions(window_dim)
        .with_min_dimensions(window_dim)
        .with_max_dimensions(window_dim)
        .with_visibility(config.visible);
    let context = glutin::ContextBuilder::new()
        .with_depth_buffer(24);
        //.with_multisampling(8);
        //.with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGlEs, (2, 0)));
    let display = glium::Display::new(window, context, &events_loop)?;
    print_context_info(&display);
    Ok((display,events_loop))
}


fn create_headless_display(config: &Config) -> Result<glium::HeadlessRenderer, Box<dyn Error>> {
    let context = glutin::HeadlessRendererBuilder::new(config.width, config.height)
        .with_gl(glutin::GlRequest::Latest)
        //.with_depth_buffer(24)
        .build()?;
    let display = glium::HeadlessRenderer::new(context)?;
    print_context_info(&display);
    Ok(display)
}


fn render_pipeline<F>(display: &F,
                      config: &Config,
                      mesh: Mesh,
                      framebuffer: &mut glium::framebuffer::SimpleFrameBuffer,
                      texture: &glium::Texture2d) -> image::DynamicImage
    where F: glium::backend::Facade,
{
    // Graphics Stuff
    // ==============

    let params = glium::DrawParameters {
        depth: glium::Depth {
            test: glium::draw_parameters::DepthTest::IfLess,
            write: true,
            .. Default::default()
        },
        backface_culling: glium::draw_parameters::BackfaceCullingMode::CullClockwise,
        .. Default::default()
    };

    // Load and compile shaders
    // ------------------------

    let vertex_shader_src = include_str!("model.vert");
    let pixel_shader_src = include_str!("model.frag");

    // TODO: Cache program binary
    let program = glium::Program::from_source(display, &vertex_shader_src, &pixel_shader_src, None);
    let program = match program {
        Ok(p) => p,
        Err(glium::CompilationError(err)) => {
            error!("{}",err);
            panic!("Compiling shaders");
        },
        Err(err) => panic!("{}",err),
    };

    // Send mesh data to GPU
    // ---------------------

    let vertex_buf = glium::VertexBuffer::new(display, &mesh.vertices).unwrap();
    let normal_buf = glium::VertexBuffer::new(display, &mesh.normals).unwrap();
    // Can use NoIndices here because STLs are dumb
    let indices = glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList);

    // Setup uniforms
    // --------------

    // Transformation matrix (positions, scales and rotates model)
    let transform_matrix = mesh.scale_and_center();

    // View matrix (convert to positions relative to camera)
    //let view_matrix = cgmath::Matrix4::look_at(CAM_POSITION, cgmath::Point3::origin(), cgmath::Vector3::unit_z());
    // These are precomputed values calculated usint the line above. We don't need to do this every time since they never change.
    // In the future it may be better to doe this automatically using const fn or something.
    let view_matrix = cgmath::Matrix4 {
        x: cgmath::Vector4 { x: 0.894, y: -0.183, z:  0.408, w: 0.000, },
        y: cgmath::Vector4 { x: 0.447, y:  0.365, z: -0.816, w: 0.000, },
        z: cgmath::Vector4 { x: 0.000, y:  0.913, z:  0.408, w: 0.000, },
        w: cgmath::Vector4 { x: 0.000, y:  0.000, z: -4.899, w: 1.000, },
    };
    debug!("View:");
    print_matrix(view_matrix.into());

    // Perspective matrix (give illusion of depth)
    let perspective_matrix = cgmath::perspective(
        cgmath::Deg(CAM_FOV_DEG),
        config.width as f32 / config.height as f32,
        0.1,
        1024.0,
    );
    debug!("Perspective:");
    print_matrix(perspective_matrix.into());

    // Direction of light source
    //let light_dir = [-1.4, 0.4, -0.7f32];
    let light_dir = [-1.1, 0.4, 1.0f32];

    let uniforms = uniform! {
        //model: Into::<[[f32; 4]; 4]>::into(transform_matrix),
        //view: Into::<[[f32; 4]; 4]>::into(view_matrix),
        modelview: Into::<[[f32; 4]; 4]>::into(view_matrix * transform_matrix),
        perspective: Into::<[[f32; 4]; 4]>::into(perspective_matrix),
        u_light: light_dir,
        ambient_color: config.material.ambient,
        diffuse_color: config.material.diffuse,
        specular_color: config.material.specular,
    };

    // Draw
    // ----

    // Fills background color and clears depth buffer
    framebuffer.clear_color_and_depth(config.background, 1.0);
    framebuffer.draw((&vertex_buf, &normal_buf), &indices, &program, &uniforms, &params)
        .unwrap();
    // TODO: Antialiasing
    // TODO: Shadows

    // Convert Image
    // =============

    let pixels: glium::texture::RawImage2d<u8> = texture.read();
    let img = image::ImageBuffer::from_raw(config.width, config.height, pixels.data.into_owned()).unwrap();
    let img = image::DynamicImage::ImageRgba8(img).flipv();

    img
}


fn show_window(display: glium::Display,
               mut events_loop: glutin::EventsLoop,
               framebuffer: glium::framebuffer::SimpleFrameBuffer,
               config: &Config) {
    // Wait until window is closed
    // ===========================

    if config.visible {
        let mut closed = false;
        let sleep_time = time::Duration::from_millis(10);
        while !closed {
            thread::sleep(sleep_time);
            // Copy framebuffer to display
            // TODO: I think theres some screwy srgb stuff going on here
            let target = display.draw();
            target.blit_from_simple_framebuffer(&framebuffer,
                                                &glium::Rect {
                                                    left: 0,
                                                    bottom: 0,
                                                    width: config.width,
                                                    height: config.height,
                                                },
                                                &glium::BlitTarget {
                                                    left: 0,
                                                    bottom: 0,
                                                    width: config.width as i32,
                                                    height: config.height as i32,
                                                },
                                                glium::uniforms::MagnifySamplerFilter::Nearest);
            target.finish().unwrap();
            // Listing the events produced by the application and waiting to be received
            events_loop.poll_events(|ev| {
                match ev {
                    glutin::Event::WindowEvent { event, .. } => match event {
                        glutin::WindowEvent::CloseRequested => closed = true,
                        glutin::WindowEvent::Destroyed => closed = true,
                        _ => (),
                    },
                    _ => (),
                }
            });
        }
    }
}

pub fn render_to_window(config: &Config) -> Result<(), Box<dyn Error>> {
    // Get geometry from STL file
    // =========================
    let stl_file = File::open(&config.stl_filename)?;
    let mesh = Mesh::from_stl(stl_file)?;

    // Create GL context
    // =================
    let (display, events_loop) = create_normal_display(&config)?;
    let texture = glium::Texture2d::empty(&display, config.width, config.height).unwrap();
    let depthtexture = glium::texture::DepthTexture2d::empty(&display, config.width, config.height).unwrap();
    let mut framebuffer = glium::framebuffer::SimpleFrameBuffer::with_depth_buffer(&display, &texture, &depthtexture).unwrap();
    render_pipeline(&display, &config, mesh, &mut framebuffer, &texture);
    show_window(display, events_loop, framebuffer, &config);

    Ok(())
}

pub fn render_to_image(config: &Config) -> Result<image::DynamicImage, Box<dyn Error>> {
    // Get geometry from STL file
    // =========================
    // TODO: Add support for URIs instead of plain file names
    // https://developer.gnome.org/integration-guide/stable/thumbnailer.html.en
    let stl_file = File::open(&config.stl_filename)?;
    let mesh = Mesh::from_stl(stl_file)?;

    let img: image::DynamicImage;

    // Create GL context
    // =================
    // 1. If not visible create a headless context.
    // 2. If headless context creation fails, create a normal context with a hidden window.
    match create_headless_display(&config) {
        Ok(display) => {
            // Note: Headless context on Linux always seems to end up with a software
            // renderer. I would prefer to try the normal context first (w/ hidden window)
            // but it panics if creating the event loop fails, which is unrecoverable.
            // Glutin is in the process of unifying the two types of contexts. Maybe then
            // headless will use hardware acceleration.
            let texture = glium::Texture2d::empty(&display, config.width, config.height).unwrap();
            let depthtexture = glium::texture::DepthTexture2d::empty(&display, config.width, config.height).unwrap();
            let mut framebuffer = glium::framebuffer::SimpleFrameBuffer::with_depth_buffer(&display, &texture, &depthtexture).unwrap();
            img = render_pipeline(&display, &config, mesh, &mut framebuffer, &texture);
        },
        Err(e) => {
            warn!("Unable to create headless GL context. Trying hidden window instead. Reason: {:?}", e);
            let (display, _) = create_normal_display(&config)?;
            let texture = glium::Texture2d::empty(&display, config.width, config.height).unwrap();
            let depthtexture = glium::texture::DepthTexture2d::empty(&display, config.width, config.height).unwrap();
            let mut framebuffer = glium::framebuffer::SimpleFrameBuffer::with_depth_buffer(&display, &texture, &depthtexture).unwrap();
            img = render_pipeline(&display, &config, mesh, &mut framebuffer, &texture);
        },
    };

    Ok(img)
}

pub fn render_to_file(config: &Config) -> Result<(), Box<dyn Error>> {
    let img = render_to_image(&config)?;

    // Output image
    // ============
    // Write to stdout if user did not specify a file
    let mut output: Box<dyn io::Write> = match config.img_filename {
        Some(ref x) => {
            Box::new(std::fs::File::create(&x).unwrap())
        },
        None => Box::new(io::stdout()),
    };
    img.write_to(&mut output, config.format.to_owned())
        .expect("Error saving image");
    
    Ok(())
}

/// Allows utilizing `stl-thumb` from C-like languages
/// 
/// This function renders an image of the file `stl_filename_c` and stores it into the buffer `buf_ptr`.
/// 
/// You must provide a memory buffer large enough to store the image. Images are written in 8-bit RGBA format,
/// so the buffer must be at least `width`*`height`*4 bytes in size. `stl_filename_c` is a pointer to a C string with
/// the file path.
/// 
/// Returns `true` if succesful and `false` if unsuccesful.
/// 
/// # Example in C
/// ```c
/// const char* stl_filename_c = "3DBenchy.stl";
/// int width = 256;
/// int height = 256;
/// 
/// int img_size = width * height * 4;
/// buf_ptr = (uchar *) malloc(img_size);
/// 
/// render_to_buffer(buf_ptr, width, height, stl_filename_c);
/// ```
#[no_mangle]
pub extern fn render_to_buffer(buf_ptr: *mut u8, width: u32, height: u32, stl_filename_c: *const c_char) -> bool {
    // Workaround for issues with OpenGL 3.1 on Mesa 18.3
    #[cfg(target_os = "linux")]
    env::set_var("MESA_GL_VERSION_OVERRIDE", "2.1");

    // Check that the buffer pointer is valid
    if buf_ptr.is_null() {
        error!("Image buffer pointer is null");
        return false;
    };
    let buf_size = (width * height * 4) as usize;
    let buf = unsafe {slice::from_raw_parts_mut(buf_ptr, buf_size) };

    // Check validity of provided file path string
    let stl_filename_cstr = unsafe {
        if stl_filename_c.is_null() {
            error!("STL file path pointer is null");
            return false;
        }
        CStr::from_ptr(stl_filename_c)
    };
    let stl_filename_str = match stl_filename_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            error!("Invalid STL file path {:?}",stl_filename_cstr);
            return false;
        },
    };

    // Setup configuration for the renderer
    let config = Config {
        stl_filename: stl_filename_str.to_string(),
        width: width,
        height: height,
        .. Default::default()
    };

    // Render

    // Run renderer in seperate thread so OpenGL problems do not crash caller
    let render_thread = thread::spawn(move || render_to_image(&config).unwrap());

    let img = match render_thread.join() {
        Ok(s) => s,
        Err(e) => {
            error!("Application error: {:?}", e);
            return false;
        },
    };

    // Copy image to output buffer
    match img.as_rgba8() {
        Some(s) => buf.copy_from_slice(s),
        None => {
            error!("Unable to get image");
            return false;
        }
    }

    true
}


// TODO: Move tests to their own file
#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::ErrorKind;
    use super::*;

    #[test]
    fn cube() {
        let img_filename = "cube.png".to_string();
        let config = Config {
            stl_filename: "test_data/cube.stl".to_string(),
            img_filename: Some(img_filename.clone()),
            format: image::ImageOutputFormat::PNG,
            .. Default::default()
        };

        match fs::remove_file(&img_filename) {
            Ok(_) => (),
            Err(ref error) if error.kind() == ErrorKind::NotFound => (),
            Err(_) => {
                panic!("Couldn't clean files before testing");
            }
        }

        render_to_file(&config).expect("Error in render function");

        let size = fs::metadata(img_filename)
            .expect("No file created")
            .len();

        assert_ne!(0, size);
    }
}
