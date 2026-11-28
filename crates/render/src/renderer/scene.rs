use super::lamp_shadows::LampDraw;
use super::*;
use rustc_hash::FxHashMap;

impl ScenePipeline {
    pub(super) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        color_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        capabilities: Capabilities,
    ) -> Self {
        let skinning = Skinning::new(&device);
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_entries = frame_layout_entries();
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &frame_entries,
        });
        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lights"),
            size: (MAX_LIGHTS * size_of::<LightRaw>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tile_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light tiles"),
            size: (MAX_TILES * (LIGHTS_PER_TILE + 1) * size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow map"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let (empty_shadow_view, empty_layers) = create_shadow_views(&device, "no shadow map", 1, 1);
        // Its one layer, as a plain 2D depth texture.
        let no_prepass_depth_view = empty_layers[0].clone();
        let no_occlusion_view = create_white_occlusion(&device, &queue);
        // The occlusion is drawn at half the scene's size, so it's
        // smoothed back up as it's read.
        let occlusion_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ambient occlusion"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let environment = environment::Environment::new(&device, &queue);
        let (environment_view, sky_image_view) = environment.views();
        let probes = probes::Probes::new(&device, &queue);
        let light_probes = light_probes::LightProbes::new(&device);
        let no_ray_shadows = ray_shadows::stand_in(&device);
        let no_ray_reflections = ray_reflections::stand_in(&device);
        let weather_field_capacity = 1usize;
        let weather_field = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("weather field"),
            size: (weather_field_capacity * size_of::<[f32; 4]>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Every animated item, morphed and skinned once a frame (CO8).
        let posing = posing::Posing::new(&device);
        // Plants' packed data per corner (foliage W3b).
        let foliage = foliage::FoliageTable::new(&device);
        let frame_bind_group = create_frame_bind_group(
            &device,
            &frame_layout,
            Some(foliage.buffer()),
            [
                &frame_buffer,
                &light_buffer,
                &tile_buffer,
                posing.posed(),
                light_probes.buffer(),
                &no_ray_shadows,
                &no_ray_reflections,
                &weather_field,
            ],
            [
                &empty_shadow_view,
                &empty_shadow_view,
                &no_occlusion_view,
                environment_view,
                sky_image_view,
                probes.view(),
                &empty_shadow_view,
                &no_prepass_depth_view,
            ],
            [&shadow_sampler, &occlusion_sampler, &environment.sampler],
        );
        let prepass_frame_bind_group = frame_bind_group.clone();
        let cascade_size = size_of::<[[f32; 4]; 4]>() as u64;
        let cascade_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow views"),
            size: CASCADE_UNIFORM_STRIDE
                * (SHADOW_CASCADES + MAX_SPOT_SHADOWS + MAX_POINT_SHADOW_VIEWS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Binding 5 matches `shadow_cascade` in shader.wgsl. Each pass picks
        // its cascade with an offset into the buffer (a "dynamic offset").
        // Bindings 0, 9 and 20 are the frame's uniform (its wind and clock,
        // for swaying plants), posed corners and plants' foliage table, as
        // in the frame's group.
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow pass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(cascade_size),
                    },
                    count: None,
                },
                skinning::posed_entry(),
                foliage_entry(),
            ],
        });
        let shadow_bind_group = create_shadow_bind_group(
            &device,
            &shadow_layout,
            &frame_buffer,
            &cascade_buffer,
            (posing.posed(), foliage.buffer()),
        );
        let shadow_pipeline = create_shadow_pipeline(&device, &shadow_layout, &skinning, None);

        // Bindings 0 and 3–9 are the textures (see `material_bindings`).
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mut entries = vec![
            texture_entry(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            // The vertex shader reads its texture coordinate transform.
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        entries.extend((3..=9).map(texture_entry));
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &entries,
        });
        // Cut-out casters' shadows, through their texture's holes.
        let shadow_cut_out_pipeline =
            create_shadow_pipeline(&device, &shadow_layout, &skinning, Some(&material_layout));
        // glTF's default: smooth, blending between mipmap levels, and
        // repeating past the edges.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let (texture_bytes, mesh_bytes) = (GpuBytes::default(), GpuBytes::default());
        let white_texture = create_texture(
            &device,
            &queue,
            &texture_bytes,
            (1, 1, &[255; 4]),
            (0, capabilities.max_texture_size),
            0,
        );
        let plain_material = create_material(
            &device,
            &material_layout,
            &sampler,
            [&white_texture; MATERIAL_TEXTURES],
            maps_uniform_of(&MaterialKey::default()),
        );

        let settings = GraphicsSettings::default();
        let graphics = settings.effective(&capabilities);
        let samples = graphics.anti_aliasing.samples();
        let cloud_trace = cloud_trace::CloudTrace::new(&device);
        let (sky_pipeline, item_pipelines, outline_pipeline, prefill_pipelines) = scene_pipelines(
            &device,
            [&frame_layout, &material_layout, &skinning.layout],
            cloud_trace.sky_layout(),
            samples,
        );
        let water = water::WaterPass::new(
            &device,
            [&frame_layout, &material_layout, &skinning.layout],
            samples,
        );
        let plant_probe = probe::PlantProbe::from_environment(
            &device,
            [&frame_layout, &material_layout, &skinning.layout],
            samples,
        );
        let depth_start = depth_start::DepthStart::new(&device);
        let mut terrain =
            terrain::TerrainPass::new(&device, &frame_layout, texture_bytes.clone(), samples);
        terrain.set_skipped_levels(graphics.texture_quality.skipped_levels());
        let sprites = sprites::SpritePass::new(&device, &white_texture.srgb, samples);
        let grid_pass = grid::GridPass::new(&device, samples);
        let decal_pass =
            decals::DecalPass::new(&device, &frame_layout, &white_texture.srgb, samples);
        let post = PostPasses::new(&device, color_format);
        let occlusion =
            OcclusionPasses::new(&device, [&frame_layout, &material_layout, &skinning.layout]);
        let edge_pipeline = create_edge_pipeline(&device, &occlusion.layout, samples);
        let temporal = temporal::TemporalPasses::new(
            &device,
            [&frame_layout, &skinning.empty_layout, &skinning.layout],
            posing.previous(),
        );
        let targets_alive = TargetsAlive::default();
        let frame_targets = FrameTargets::new(
            &device,
            (&post, &occlusion, &temporal),
            scene_size((width, height), &graphics, &capabilities),
            (width, height),
            targets_used(&graphics, false, false, false),
            &targets_alive,
        );
        let ui_pipeline = create_ui_pipeline(&device, color_format);
        let (ui_image_layout, ui_sampler, ui_image_pipeline, ui_sheet_pipeline) =
            create_ui_image_pipeline(&device, color_format);
        let meshes = Meshes {
            block: upload_mesh(&device, &mesh_bytes, "block", &mesh::block()),
            sphere: upload_mesh(&device, &mesh_bytes, "sphere", &mesh::sphere()),
            cylinder: upload_mesh(&device, &mesh_bytes, "cylinder", &mesh::cylinder()),
            wedge: upload_mesh(&device, &mesh_bytes, "wedge", &mesh::wedge()),
            corner_wedge: upload_mesh(&device, &mesh_bytes, "corner wedge", &mesh::corner_wedge()),
        };
        let instance_capacity = 64;
        let instance_buffer = create_instance_buffer(&device, instance_capacity);
        let ui_capacity = 128;
        let ui_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI vertices"),
            size: (ui_capacity * size_of::<UiVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut font_system = FontSystem::new();
        for data in crate::fonts::built_in_data() {
            font_system.db_mut().load_font_data(data.to_vec());
        }
        let swash_cache = SwashCache::new();
        let glyph_cache = GlyphCache::new(&device);
        let text_viewport = TextViewport::new(&device, &glyph_cache);
        let mut text_atlas = TextAtlas::new(&device, &queue, &glyph_cache, color_format);
        let turned_text = TurnedText {
            renderer: GlyphTextRenderer::new(
                &mut text_atlas,
                &device,
                wgpu::MultisampleState::default(),
                None,
            ),
            viewport: TextViewport::new(&device, &glyph_cache),
            sheet: None,
        };
        let gpu_timer = gpu_timer::GpuTimer::new(&device, &queue);

        let mut pipeline = Self {
            device,
            queue,
            sky_pipeline,
            item_pipelines,
            prefill_pipelines,
            plant_probe,
            depth_start,
            // `AXCENITY_NO_WHOLE_SOLIDS` lights them with `fs_main` as
            // before, for comparing the two.
            whole_solids: std::env::var_os("AXCENITY_NO_WHOLE_SOLIDS").is_none(),
            cut_out_cascades: lighting::cut_out_cascades(),
            cascades_drawn: [None; SHADOW_CASCADES],
            casters_planned: 0,
            outline_pipeline,
            edge_pipeline,
            edges: false,
            decals: false,
            sun_rays: false,
            particle_collisions: false,
            sun_rays_pass: None,
            depth_of_field: false,
            motion_blur: false,
            temporal,
            history: Default::default(),
            surface_shaders: Default::default(),
            sprites,
            decal_pass,
            grid_pass,
            post,
            frame_targets,
            occlusion,
            atmosphere: Default::default(),
            environment,
            probes,
            light_probes,
            realtime_gi: None,
            volumetric_fog: None,
            gpu_particles: None,
            water,
            water_frame_bind_group: None,
            cloud_drift: Default::default(),
            cloud_trace,
            frame_entries,
            sun_disc: true,
            sky_band0: glam::Vec3::ZERO,
            no_occlusion_view,
            occlusion_sampler,
            targets_alive,
            output_size: (width, height),
            light_buffer,
            tile_buffer,
            ui_pipeline,
            ui_image_pipeline,
            ui_sheet_pipeline,
            ui_image_layout,
            ui_sampler,
            ui_image_groups: FxHashMap::default(),
            ui_buffer,
            ui_capacity,
            font_system,
            loaded_fonts: HashSet::new(),
            fitted_sizes: HashMap::new(),
            swash_cache,
            text_viewport,
            text_atlas,
            text_renderers: Vec::new(),
            turned_text,
            color_format,
            frame_buffer,
            frame_bind_group,
            prepass_frame_bind_group,
            frame_layout,
            empty_shadow_view,
            no_prepass_depth_view,
            shadow_sampler,
            shadow_pipeline,
            shadow_cut_out_pipeline,
            cascade_buffer,
            shadow_layout,
            shadow_bind_group,
            skinning,
            shadow_map: None,
            shadow_maps_alive: Arc::new(()),
            spot_shadow_map: None,
            spot_shadow_maps_alive: Arc::new(()),
            point_shadow_map: None,
            point_shadow_maps_alive: Arc::new(()),
            graphics,
            settings,
            capabilities,
            meshes,
            instance_buffer,
            instance_capacity,
            material_layout,
            sampler,
            white_texture,
            plain_material,
            uploader: None,
            skirts: Default::default(),
            frame_clock: std::collections::VecDeque::new(),
            custom_meshes: FxHashMap::default(),
            textures: FxHashMap::default(),
            full_textures: FxHashMap::default(),
            materials: FxHashMap::default(),
            terrain,
            frame: 0,
            gpu_timer,
            latency: None,
            cpu_times: [Duration::ZERO; CpuSection::COUNT],
            one_frame_ahead: false,
            frame_in_parts: encoders::in_parts_by_default(),
            last_frame: None,
            gpu_wait: Duration::ZERO,
            time_gpu: true,
            counts: FrameCounts::default(),
            frozen_view: None,
            culling_bounds: Vec::new(),
            lods: LodStates::default(),
            clusters: clusters::Clusters::default(),
            ray_scene: None,
            parked_ray_scene: None,
            posing_generation: posing.generation,
            posing,
            foliage_generation: foliage.generation,
            foliage,
            ray_shadows: None,
            no_ray_shadows,
            ray_reflections: None,
            screen_reflections: None,
            no_ray_reflections,
            weather_field,
            weather_field_capacity,
            weather_prints_uploaded: None,
            weather_print_bytes: 0,
            ray_ao: None,
            ray_gi: None,
            soft_scene: None,
            #[cfg(feature = "debug-views")]
            debug_views: None,
            texture_bytes,
            mesh_bytes,
            keep_undrawn: false,
            freed_for_budget: 0,
        };
        pipeline.bind_frame();
        pipeline
    }

    /// Uses `settings` from the next frame, lowered to what the device can
    /// do. A shadow map (the sun's, the spot lights' or the point lights')
    /// of the wrong size is
    /// freed here and remade at the right size when next drawn. A new render
    /// scale remakes the scene's targets, and a new texture quality frees
    /// every texture drawn in the scene (and the materials using them), so
    /// they're uploaded again at the new size. A new anti-aliasing remakes
    /// the targets and, if its samples per pixel changed, the scene pass's
    /// pipelines.
    pub(super) fn set_graphics(&mut self, settings: &GraphicsSettings) {
        if *settings != self.settings {
            self.apply_graphics(settings);
        }
    }

    /// Draws as if the device could do no more than `limits`: each value is
    /// the lower of the two, so nothing is raised. The settings are lowered
    /// again to fit, and textures are uploaded again within the new size.
    pub(super) fn limit_capabilities(&mut self, limits: &Capabilities) {
        let real = self.capabilities;
        self.capabilities = Capabilities {
            max_texture_size: real.max_texture_size.min(limits.max_texture_size),
            max_texture_layers: real.max_texture_layers.min(limits.max_texture_layers),
            timestamp_queries: real.timestamp_queries && limits.timestamp_queries,
            software: real.software || limits.software,
            msaa_samples: real.msaa_samples & limits.msaa_samples,
            compute_shaders: real.compute_shaders && limits.compute_shaders,
            ray_queries: real.ray_queries && limits.ray_queries,
            ray_tracing_offered: real.ray_tracing_offered && limits.ray_tracing_offered,
            // Limits that don't name a memory leave the presets' budgets.
            video_memory: real
                .video_memory
                .zip(limits.video_memory)
                .map(|(real, limit)| real.min(limit)),
        };
        let settings = self.settings.clone();
        self.apply_graphics(&settings);
        self.materials.clear();
        self.textures.clear();
        self.sprites.forget(|_| false);
        self.decal_pass.forget(|_| false);
    }

    /// Uses `settings` from the next frame (see [`Self::set_graphics`]).
    fn apply_graphics(&mut self, settings: &GraphicsSettings) {
        let old_quality = self.graphics.texture_quality;
        let old_samples = self.graphics.anti_aliasing.samples();
        self.settings = settings.clone();
        self.graphics = settings.effective(&self.capabilities);
        // Counted against the budgets in use: new ones start again.
        self.freed_for_budget = 0;
        if !self.graphics.realtime_gi && self.realtime_gi.take().is_some() {
            // The probes traced while it was on give way to their bakes.
            self.light_probes.restore_bakes();
        }
        // A new tier (roadmap R18.4) is made afresh, keeping what the old
        // one traced until it's traced again.
        if self
            .realtime_gi
            .as_ref()
            .is_some_and(|gi| gi.tier != self.graphics.realtime_gi_tier)
        {
            self.realtime_gi = None;
        }
        if !ray_gi::traced(&self.graphics, 1) && self.ray_gi.take().is_some() {
            self.light_probes.restore_bakes();
        }
        if !self.graphics.volumetric_fog {
            self.volumetric_fog = None;
        }
        // New settings (ray traced shadows on or off, a new shadow quality)
        // light the fog differently: its grid starts again rather than
        // fade from the old look.
        if let Some(fog) = &mut self.volumetric_fog {
            fog.forget_history();
        }
        // The Path Traced view's pass comes and goes with ray queries.
        #[cfg(feature = "debug-views")]
        if let Some(view) = self.debug_views.as_ref().map(|debug| debug.view) {
            self.set_debug_view(Some(view));
        }
        let quality = self.graphics.shadow_quality;
        if self.shadow_map.as_ref().map(|map| map.size) != quality.map() {
            self.shadow_map = None;
        }
        if self.spot_shadow_map.as_ref().map(|map| map.size) != quality.spot_map() {
            self.spot_shadow_map = None;
        }
        if self.point_shadow_map.as_ref().map(|map| map.size) != quality.point_map() {
            self.point_shadow_map = None;
        }
        self.bind_frame();
        let samples = self.graphics.anti_aliasing.samples();
        if samples != old_samples {
            let building = std::time::Instant::now();
            (
                self.sky_pipeline,
                self.item_pipelines,
                self.outline_pipeline,
                self.prefill_pipelines,
            ) = scene_pipelines(
                &self.device,
                [
                    &self.frame_layout,
                    &self.material_layout,
                    &self.skinning.layout,
                ],
                self.cloud_trace.sky_layout(),
                samples,
            );
            self.edge_pipeline =
                create_edge_pipeline(&self.device, &self.occlusion.layout, samples);
            self.plant_probe = probe::PlantProbe::from_environment(
                &self.device,
                [
                    &self.frame_layout,
                    &self.material_layout,
                    &self.skinning.layout,
                ],
                samples,
            );
            self.terrain
                .set_samples(&self.device, &self.frame_layout, samples);
            self.water.set_samples(
                &self.device,
                [
                    &self.frame_layout,
                    &self.material_layout,
                    &self.skinning.layout,
                ],
                samples,
            );
            self.sprites.set_samples(&self.device, samples);
            self.decal_pass.set_samples(&self.device, samples);
            self.grid_pass.set_samples(&self.device, samples);
            self.surface_shaders.clear_pipelines();
            log::info!(
                "graphics pipelines rebuilt for {samples} samples per pixel in {} ms",
                building.elapsed().as_millis()
            );
        }
        self.fit_targets();
        if self.graphics.texture_quality != old_quality {
            self.materials.clear();
            self.textures.clear();
            self.sprites.forget(|_| false);
            self.decal_pass.forget(|_| false);
            self.terrain
                .set_skipped_levels(self.graphics.texture_quality.skipped_levels());
        }
    }

    /// Call when the target changes size. Sizes are in pixels and never zero.
    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.output_size = (width, height);
        self.fit_targets();
    }

    /// See [`crate::TextureRenderer::forget_history`].
    pub(super) fn forget_history(&mut self) {
        self.history.reset();
        if let Some(fog) = &mut self.volumetric_fog {
            fog.forget_history();
        }
    }

    /// Remakes the scene's targets if they weren't made for this output
    /// size, render scale, anti-aliasing, ambient occlusion, contact
    /// shadows, volumetric fog, screen-space outlines, depth of field, TAA
    /// and motion blur. The old ones are freed, and TAA's history with them.
    fn fit_targets(&mut self) {
        let size = scene_size(self.output_size, &self.graphics, &self.capabilities);
        let used = targets_used(
            &self.graphics,
            self.edges || self.decals || self.sun_rays || self.particle_collisions,
            self.depth_of_field,
            self.motion_blur,
        );
        let targets = &self.frame_targets;
        let made_for = (
            targets.size,
            targets.output,
            (
                targets.samples,
                targets.fxaa.is_some(),
                targets.occlusion.is_some(),
                targets.depth_of_field.is_some(),
                targets
                    .temporal
                    .as_ref()
                    .map_or_else(Default::default, |temporal| temporal.used()),
            ),
        );
        if made_for != (size, self.output_size, used) {
            self.frame_targets = FrameTargets::new(
                &self.device,
                (&self.post, &self.occlusion, &self.temporal),
                size,
                self.output_size,
                used,
                &self.targets_alive,
            );
            self.history.valid = false;
            // Last frame's picture went with them.
            if let Some(pass) = &mut self.screen_reflections {
                pass.previous = None;
            }
            // The frame bindings held the old occlusion texture, and so did
            // the traced clouds'.
            self.cloud_trace.forget_target();
            self.bind_frame();
        }
    }

    /// Draws `list` into `view`, which must be the size last given to
    /// `new` or `resize`, and sends the work to the GPU.
    pub(super) fn draw(
        &mut self,
        list: &DrawList,
        view: &wgpu::TextureView,
        aspect: f32,
        viewport_size: (u32, u32),
    ) -> Result<()> {
        let mut cpu = cpu_timer::CpuTimer::start();
        // Screen-space outlines keep the prepass's targets alive while the
        // world has them.
        // So does depth of field, with a target of its own, and motion blur,
        // with the motion targets.
        let depth_of_field = lens::wants_depth_of_field(&list.lens);
        let motion_blur = temporal::wants_motion_blur(&list.lens);
        // So do projected decals, while the settings draw any.
        let decals = !list.decals.is_empty() && self.graphics.decals.count > 0;
        if self.decals != decals {
            self.decals = decals;
            self.fit_targets();
            // The frame bindings hand decal.wgsl the prepass's depth only
            // while it's read.
            self.bind_frame();
        }
        // And sun rays, which tell sky from solid by its depth.
        let sun_rays = sun_rays::wanted(list, &self.graphics);
        // And GPU particles that land on what it shows (roadmap R17.4b).
        let particle_collisions = self.capabilities.compute_shaders
            && list
                .particle_emitters
                .iter()
                .any(|emitter| emitter.collision.is_some());
        if self.edges != list.edges.is_some()
            || self.sun_rays != sun_rays
            || self.particle_collisions != particle_collisions
            || self.depth_of_field != depth_of_field
            || self.motion_blur != motion_blur
        {
            self.edges = list.edges.is_some();
            self.sun_rays = sun_rays;
            self.particle_collisions = particle_collisions;
            self.depth_of_field = depth_of_field;
            self.motion_blur = motion_blur;
            self.fit_targets();
        }
        // Lights are planned in the scene's pixels, which the render scale
        // makes fewer or more than the output's. UI uses the output's.
        let quality = self.graphics.shadow_quality;
        let planned_lights = plan_lights(list, aspect, self.frame_targets.size, quality);
        // An atmosphere's sky is worked out, and the sky's maps baked, only
        // when the sky has changed.
        let maps = self.graphics.reflection_quality.map().is_some();
        let sky = self.atmosphere.look(&list.sky, maps);
        if self.environment.prepare(
            &self.device,
            &self.queue,
            &sky,
            self.graphics.reflection_quality,
            self.capabilities.max_texture_size,
        ) {
            self.bind_frame();
        }
        // So are the reflection probes, each only when it's new.
        if self.probes.prepare(
            (&self.device, &self.queue),
            &self.environment,
            &list.reflection_probes,
            list.camera.position,
            (
                self.graphics.reflection_quality,
                self.capabilities.max_texture_size,
            ),
        ) {
            self.bind_frame();
        }
        // And the light probe volumes, each only when it's new, with the
        // camera's own grids of probes, out to the rays' reach, while ray
        // traced GI is on.
        if self.light_probes.prepare(
            (&self.device, &self.queue),
            &list.light_probe_volumes,
            list.camera.position,
            ray_gi::wanted(&self.graphics).then_some(self.graphics.ray_tracing_distance),
        ) {
            self.bind_frame();
        }
        // With ray traced GI (renderer/ray_gi.rs), a batch of their probes
        // is traced with rays this frame, once the ray traced scene is up
        // to date; its pass needs the frame's bindings.
        let ray_gi = ray_gi::traced(&self.graphics, self.light_probes.count());
        if !ray_gi {
            if self.ray_gi.take().is_some() {
                // The probes traced while it was on give way to their bakes.
                self.light_probes.restore_bakes();
            }
        } else if self
            .ray_gi
            .as_ref()
            .is_none_or(|gi| gi.software != self.graphics.ray_traced_gi_in_software)
        {
            // Its rays are traced in hardware, or in software (RL3).
            self.ray_gi = Some(Box::new(ray_gi::RayGlobalIllumination::new(
                &self.device,
                &self.frame_entries,
                self.graphics.ray_traced_gi_in_software,
            )));
            self.bind_frame();
        }
        let ray_gi_batch = match &mut self.ray_gi {
            Some(gi) => gi.plan(&self.queue, &self.light_probes, &self.graphics),
            None => None,
        };
        // Otherwise, with real-time GI, a few of their probes are traced
        // again first by drawing what they see.
        if !ray_gi && self.graphics.realtime_gi && self.light_probes.count() > 0 {
            let gi = self.realtime_gi.get_or_insert_with(|| {
                Box::new(realtime_gi::RealtimeGi::new(
                    &self.device,
                    &self.queue,
                    list,
                    &self.settings,
                ))
            });
            gi.update(list, &self.light_probes, &self.settings)?;
        } else {
            self.realtime_gi = None;
        }
        let mut frame_uniform = frame_uniform(list, aspect, &self.graphics);
        // The time and wind plants sway by, kept for next frame's motion.
        let clock = frame_uniform.clock;
        // With TAA, the scene is drawn a fraction of a pixel to one side,
        // a different way each frame (renderer/temporal.rs).
        let view_proj = camera_view_projection(&list.camera, aspect);
        // Upscaling TAA steps through more places, since each output pixel
        // gets fewer of the scene's.
        let taa_upscales = self
            .frame_targets
            .temporal
            .as_ref()
            .is_some_and(|temporal| temporal.upscales());
        let jitter_phases = if taa_upscales {
            temporal::jitter_phases(
                self.frame_targets.size.0 as f32 / self.frame_targets.output.0.max(1) as f32,
            )
        } else {
            temporal::JITTER_PHASES
        };
        let jitter = self.history.jitter(jitter_phases);
        let drawn_view_proj = match &self.frame_targets.temporal {
            Some(temporal) if temporal.has_taa() => {
                self.history
                    .jittered(view_proj, self.frame_targets.size, jitter_phases)
            }
            _ => view_proj,
        };
        frame_uniform.view_proj = drawn_view_proj.to_cols_array_2d();
        frame_uniform.inv_view_proj = drawn_view_proj.inverse().to_cols_array_2d();
        frame_uniform.noise[0] = self.shading_noise_step();
        // Hair cards' hashed alpha (hair.wgsl) only while TAA averages it.
        frame_uniform.noise[1] = f32::from(u8::from(self.taa_on()));
        (frame_uniform.environment, frame_uniform.sky_light) = self.environment.uniform();
        let sky_colors = [
            &mut frame_uniform.sky_top,
            &mut frame_uniform.sky_horizon,
            &mut frame_uniform.sky_ground,
        ];
        for (slot, color) in sky_colors.into_iter().zip(sky.colors) {
            slot[..3].copy_from_slice(&color.to_array());
        }
        let stars = if list.sky.stars.is_finite() {
            list.sky.stars.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // A creator's sky picture dims at night as the colors do; an
        // atmosphere's works out its own light.
        let picture_light =
            if sky.image.is_some() && !sky.scattered && list.sky.daylight.is_finite() {
                list.sky.daylight.clamp(0.0, 1.0)
            } else {
                1.0
            };
        for coefficient in &mut frame_uniform.sky_light {
            for channel in &mut coefficient[..3] {
                *channel *= picture_light;
            }
        }
        self.sky_band0 = self.environment.sky_band0(sky.colors, picture_light);
        frame_uniform.night = [
            stars,
            f32::from(u8::from(sky.scattered)),
            picture_light,
            0.0,
        ];
        // Volumetric fog needs the prepass's depth, which its setting keeps.
        let volumetric = fog::volumetric_drawn(&list.fog, &self.graphics)
            && self.frame_targets.occlusion.is_some();
        if !volumetric {
            self.volumetric_fog = None;
        } else if self.volumetric_fog.is_none() {
            self.volumetric_fog = Some(Box::new(fog::VolumetricFog::new(
                &self.device,
                &self.frame_entries,
                &self.frame_layout,
                &self.targets_alive.volumetric_fog,
            )));
            self.bind_frame();
        }
        // The sky's light averaged over every way (as `sky_diffuse` sees it,
        // times the environment intensity), which lights dust where no
        // volumetric fog or light probes say how open it is.
        let sky_average = {
            let rgb = |slot: &[f32; 4]| glam::Vec3::from_slice(&slot[..3]);
            let average = if frame_uniform.environment[0] > 0.5 {
                rgb(&frame_uniform.sky_light[0]) * 0.282_095
            } else {
                let sky = rgb(&frame_uniform.sky_horizon).lerp(rgb(&frame_uniform.sky_top), 0.5);
                (sky + rgb(&frame_uniform.sky_ground)) * 0.5
            };
            average * frame_uniform.sky_top[3]
        };
        frame_uniform.fog = fog::FogRaw::new(
            &list.fog,
            sky.fog_horizon,
            sky_average,
            glam::Vec3::from_slice(&frame_uniform.sun_direction[..3]),
            glam::Vec3::from_slice(&frame_uniform.sun_radiance[..3]),
            if volumetric {
                fog::VOLUMETRIC_REACH
            } else {
                0.0
            },
        );
        // The grid's history: which grid to write, where last frame's
        // camera was, and how this frame's points shift in each cell.
        if let Some(fog) = &mut self.volumetric_fog {
            frame_uniform.fog.dust_count[2] = fog.begin_frame(
                &self.queue,
                fog::FogCamera {
                    view_proj: drawn_view_proj,
                    position: glam::Vec3::from_slice(&frame_uniform.camera_position[..3]),
                },
            );
        }
        let drift = self.cloud_drift.advance(&list.clouds, list.time);
        frame_uniform.clouds =
            clouds::CloudsRaw::new(&list.clouds, drift, self.graphics.cloud_quality);
        self.cloud_trace.plan(
            (&self.device, &self.queue),
            &mut frame_uniform,
            drift.map(f64::from),
            self.frame_targets.size,
            self.frame_targets
                .occlusion
                .as_ref()
                .map(|occlusion| &occlusion.depth_view),
        );
        let (probes, probe_count) = self.probes.uniform();
        frame_uniform.probes = probes;
        frame_uniform.reflection_probes[0] = probe_count;
        // Water's copy of the solid scene exists only while it's used.
        let water_mode = water::WaterMode::of(list, &self.graphics);
        frame_uniform.reflection_probes[1..3].copy_from_slice(&water_mode.flags());
        let copy_size = water_mode.copies().then_some(self.frame_targets.size);
        if self
            .water
            .fit(&self.device, copy_size, &self.targets_alive.water)
        {
            self.bind_frame();
        }
        let (volumes, volume_count) = self.light_probes.uniform(self.sky_band0);
        frame_uniform.volumes = volumes;
        frame_uniform.light_probes = [volume_count, u32::from(!self.sun_disc), 0, 0];
        let (scene_width, scene_height) = self.frame_targets.size;
        frame_uniform.ambient_occlusion[2] = 1.0 / scene_width as f32;
        frame_uniform.ambient_occlusion[3] = 1.0 / scene_height as f32;
        frame_uniform.lights = [
            planned_lights.lights.len() as u32,
            planned_lights.tile_size,
            planned_lights.tiles_across,
            0,
        ];
        for (matrix, spot) in frame_uniform
            .spot_shadows
            .iter_mut()
            .zip(&planned_lights.spot_shadows)
        {
            *matrix = spot.view_proj.to_cols_array_2d();
        }
        let point_faces = planned_lights
            .point_shadows
            .iter()
            .flat_map(|point| point.faces);
        for (matrix, face) in frame_uniform.point_shadows.iter_mut().zip(point_faces) {
            *matrix = face.to_cols_array_2d();
        }
        // Far cascades of a heavy scene take turns being drawn: those not
        // drawn this frame keep what they last drew, and the matrix it was
        // drawn with goes in `frame_uniform`.
        // Ray traced sun shadows, while drawn (renderer/ray_shadows.rs):
        // their mask is bound in the frame's place 17.
        let ray_shadow_uniform = match ray_shadows::RayShadowPlan::of(list, &self.graphics)
            .filter(|_| self.frame_targets.occlusion.is_some())
        {
            None => {
                if self.ray_shadows.take().is_some() {
                    self.bind_frame();
                }
                None
            }
            Some(mut plan) => {
                // The fog's air is traced too while its grid is drawn (LE3).
                plan.air = volumetric;
                let size = self.frame_targets.size;
                if !self
                    .ray_shadows
                    .as_ref()
                    .is_some_and(|shadows| shadows.fits(size, plan.shape()))
                {
                    self.ray_shadows = Some(Box::new(ray_shadows::RayShadows::new(
                        &self.device,
                        (size, plan.shape()),
                    )));
                    self.bind_frame();
                }
                let lamp_words = self
                    .ray_shadows
                    .as_ref()
                    .map_or(0, |shadows| shadows.lamp_words());
                frame_uniform.ray_shadows = [1.0, size.0 as f32, plan.reach, lamp_words as f32];
                frame_uniform.fog.dust_count[1] = self
                    .ray_shadows
                    .as_ref()
                    .map_or(0, |shadows| shadows.air_words());
                // Under TAA the rays turn each frame, and TAA averages them.
                let taa = self
                    .frame_targets
                    .temporal
                    .as_ref()
                    .is_some_and(|temporal| temporal.has_taa());
                let turn = if taa { (self.frame % 64) as f32 } else { 0.0 };
                Some(ray_shadows::ray_shadow_uniform(
                    list,
                    &plan,
                    (
                        drawn_view_proj.inverse(),
                        glam::Mat4::from_cols_array_2d(&frame_uniform.view),
                    ),
                    (
                        size,
                        if planned_lights.lights.is_empty() {
                            (planned_lights.tile_size, 0)
                        } else {
                            (planned_lights.tile_size, planned_lights.tiles_across)
                        },
                    ),
                    (turn, frame_uniform.fog.dust_count[2]),
                ))
            }
        };
        // Ray traced ambient occlusion, while drawn (renderer/ray_ao.rs).
        let ray_ao_uniform =
            if ray_ao::traced(&self.graphics) && self.frame_targets.occlusion.is_some() {
                if self.ray_ao.is_none() {
                    self.ray_ao = Some(Box::new(ray_ao::RayAmbientOcclusion::new(
                        &self.device,
                        &self.occlusion,
                    )));
                }
                let taa = self
                    .frame_targets
                    .temporal
                    .as_ref()
                    .is_some_and(|temporal| temporal.has_taa());
                let turn = if taa { (self.frame % 64) as f32 } else { 0.0 };
                let (view, _) = lighting::camera_matrices(&list.camera, aspect);
                Some(ray_ao::ray_ao_uniform(list, &self.graphics, view, turn))
            } else {
                self.ray_ao = None;
                None
            };
        // Ray traced reflections, while drawn (renderer/ray_reflections.rs):
        // what their rays find is bound in the frame's place 18.
        let ray_reflection_uniform =
            if ray_reflections::traced(&self.graphics) && self.frame_targets.occlusion.is_some() {
                let size = self.frame_targets.size;
                let downscale = self.graphics.ray_traced_reflections.downscale();
                if !self
                    .ray_reflections
                    .as_ref()
                    .is_some_and(|pass| pass.fits(size, downscale))
                {
                    self.ray_reflections = Some(Box::new(ray_reflections::RayReflections::new(
                        &self.device,
                        &self.frame_entries,
                        (size, downscale),
                    )));
                    self.bind_frame();
                }
                let view = glam::Mat4::from_cols_array_2d(&frame_uniform.view);
                self.ray_reflections.as_ref().map(|pass| {
                    frame_uniform.ray_reflections = pass.frame_values(&self.graphics);
                    pass.uniform(&self.graphics, drawn_view_proj.inverse(), view)
                })
            } else {
                if self.ray_reflections.take().is_some() {
                    self.bind_frame();
                }
                None
            };
        // Screen-space reflections, while drawn and ray traced ones aren't
        // (renderer/screen_reflections.rs): what they find on the screen is
        // bound in the frame's place 18, as ray traced reflections are. Not
        // on the first frame drawn into these targets: there is no last
        // frame's picture to read.
        let screen_reflection_uniform = if screen_reflections::marched(&self.graphics)
            && self.frame_targets.occlusion.is_some()
        {
            let size = self.frame_targets.size;
            if !self
                .screen_reflections
                .as_ref()
                .is_some_and(|pass| pass.fits(size))
            {
                self.screen_reflections = Some(Box::new(
                    screen_reflections::ScreenReflections::new(&self.device, size),
                ));
                self.bind_frame();
            }
            let matrices = lighting::camera_matrices(&list.camera, aspect);
            self.screen_reflections.as_ref().and_then(|pass| {
                // On from the first frame, for the sky's shading: with no
                // last frame to read, the pass's buffer is cleared instead.
                frame_uniform.ray_reflections = pass.frame_values();
                // Its steps start at a different offset each frame only while
                // TAA averages them (as the shading's noise).
                let step = frame_uniform.noise[0] as u64;
                pass.uniform(drawn_view_proj, matrices, step)
            })
        } else {
            if self.screen_reflections.take().is_some() {
                self.bind_frame();
            }
            None
        };
        // This frame's picture is next frame's to read.
        if let Some(pass) = &mut self.screen_reflections {
            pass.previous = Some(view_proj);
        }
        if let Some(field) = list
            .weather_field
            .as_ref()
            .filter(|field| !list.weather.is_clear() && field.is_valid())
        {
            // The snow's print map goes after the field's cells, four of
            // its cells to an entry.
            let prints = field
                .prints
                .as_ref()
                .filter(|prints| prints.cells.is_valid() && prints.fill.is_finite());
            let print_entries = prints.map_or(0, |prints| prints.cells.cells.len() / 4);
            let entries = field.cells.len() + print_entries;
            if entries > self.weather_field_capacity {
                self.weather_field_capacity = entries;
                self.weather_field = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("weather field"),
                    size: (self.weather_field_capacity * size_of::<[f32; 4]>()) as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.weather_prints_uploaded = None;
                self.bind_frame();
            }
            self.queue
                .write_buffer(&self.weather_field, 0, bytemuck::cast_slice(&field.cells));
            frame_uniform.weather[3] = 1.0;
            frame_uniform.weather_bounds = [
                field.origin[0],
                field.origin[1],
                field.cell_size,
                field.size as f32,
            ];
            match prints {
                Some(prints) => {
                    self.upload_weather_prints(&prints.cells, field.cells.len());
                    frame_uniform.weather_prints = [
                        prints.cells.origin[0],
                        prints.cells.origin[1],
                        prints.cells.cells_per_stud,
                        prints.cells.size as f32,
                    ];
                    frame_uniform.precipitation[3] = prints.fill;
                }
                None => self.weather_prints_uploaded = None,
            }
        }
        let cascade_drawn = self.schedule_cascades(list, &mut frame_uniform, aspect);
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&frame_uniform));
        // The sun's cascades, then the spot lights' views, then the point
        // lights' faces.
        let shadow_views = frame_uniform
            .cascades
            .iter()
            .chain(&frame_uniform.spot_shadows)
            .chain(&frame_uniform.point_shadows);
        for (index, matrix) in shadow_views.enumerate() {
            self.queue.write_buffer(
                &self.cascade_buffer,
                index as u64 * CASCADE_UNIFORM_STRIDE,
                bytemuck::bytes_of(matrix),
            );
        }
        if !planned_lights.lights.is_empty() {
            self.queue.write_buffer(
                &self.light_buffer,
                0,
                bytemuck::cast_slice(&planned_lights.lights),
            );
            self.queue.write_buffer(
                &self.tile_buffer,
                0,
                bytemuck::cast_slice(&planned_lights.tiles),
            );
        }
        // Ambient occlusion is drawn screen-space, ray traced or both (the
        // rays over screen-space's, fading into it toward their reach).
        let screen_space_occlusion =
            self.graphics.ambient_occlusion != crate::AmbientOcclusion::Off;
        let ambient_occlusion = screen_space_occlusion || ray_ao_uniform.is_some();
        if ambient_occlusion && self.frame_targets.occlusion.is_some() {
            let uniform = occlusion_uniform(list, aspect, self.graphics.ambient_occlusion);
            self.queue
                .write_buffer(&self.occlusion.uniform, 0, bytemuck::bytes_of(&uniform));
        }
        if let Some(edges) = &list.edges {
            let uniform = edge_uniform(list, edges, aspect, self.frame_targets.size);
            self.queue.write_buffer(
                &self.occlusion.edge_uniform,
                0,
                bytemuck::bytes_of(&uniform),
            );
        }
        let post = post_drawn(&list.post, &self.graphics);
        let mut post_settings = post_uniform(&post);
        lens::add_lens(
            &mut post_settings,
            &list.lens,
            &self.graphics,
            (aspect, self.output_size.1),
            self.frame,
        );
        self.queue
            .write_buffer(&self.post.settings, 0, bytemuck::bytes_of(&post_settings));
        if self.frame_targets.depth_of_field.is_some() {
            self.post.depth_of_field.prepare(
                &self.queue,
                list,
                aspect,
                self.frame_targets.size.1,
                self.graphics.depth_of_field,
            );
        }
        self.post
            .grading
            .prepare(&self.device, &self.queue, list.grading.as_ref());

        cpu.lap(CpuSection::Setup);
        self.frame += 1;
        // Each item's level of detail, which decides the meshes uploaded.
        // Everything the frame's views can see, or everything while a
        // frozen view is being looked at from outside.
        let reach = self.frozen_view.is_none().then(|| {
            let mut reach = Reach::default();
            reach
                .views
                .push(Frustum::of(camera_view_projection(&list.camera, aspect)));
            let cascades = shadows_drawn(list.shadows, &self.graphics)
                .map_or(0, |(_, size)| size.cascades as usize);
            // Not the cascades ray traced shadows leave out.
            let under_rays = super::lighting::cascades_under_rays(list, &self.graphics, aspect);
            reach.views.extend(super::lighting::cascade_views(
                &frame_uniform.cascades[..cascades],
                under_rays,
            ));
            // Ray traced shadows' rays toward the sun, where the shadow
            // maps reach less far than they do (or aren't drawn).
            if let Some(rays) = ray_shadows::RayShadowPlan::of(list, &self.graphics)
                && shadows_drawn(list.shadows, &self.graphics)
                    .is_none_or(|(maps, _)| maps.distance < rays.reach)
            {
                reach
                    .views
                    .push(Frustum::of(sun_reach(list, rays.reach, aspect)));
            }
            for spot in &planned_lights.spot_shadows {
                reach.lamps.push((spot.bounds, Frustum::of(spot.view_proj)));
            }
            for point in &planned_lights.point_shadows {
                for face in point.faces {
                    reach.lamps.push((point.bounds, Frustum::of(face)));
                }
            }
            reach
        });
        let Resolved {
            draws,
            clusters: groups,
            culled: past_cull_distance,
            submitted: items_submitted,
        } = self.lods.resolve(list, self.frame, reach.as_ref());
        // What each group of items is was worked out when it first came
        // into a view. The clusters are taken out of the pipeline while
        // the frame reads them, so its other borrows can stay (a frame
        // that fails loses them, and the next works them out again).
        let mut clusters = std::mem::take(&mut self.clusters);
        if self.frame.is_multiple_of(60) {
            clusters.forget_unused(self.frame);
        }
        let groups = clusters.of(&groups, self.frame);
        cpu.lap(CpuSection::Lods);
        // Each draw's material binding, worked out once for the uploads
        // and the plan: it reads every map setting of the item.
        let mut uses = FrameUses::of(&draws, &groups);
        if self.graphics.shadow_lod {
            // The farthest cascade may draw a caster at a simpler level
            // than the camera does: those copies go up too.
            uses.add_lods(&draws, &groups, self.frame);
        }
        cpu.lap(CpuSection::Materials);
        self.upload_new(list, &draws, &uses);
        cpu.lap(CpuSection::Uploads);
        // What the camera can see, or the frozen view's while it's set.
        let camera = self.frozen_view.as_ref().unwrap_or(&list.camera);
        let camera_view = Frustum::of(camera_view_projection(camera, aspect));
        let mut seen = std::mem::take(&mut self.culling_bounds);
        seen.clear();
        let DrawPlan {
            mut instances,
            batches,
            casters,
            culled,
            mut joints,
            skins,
            posed,
            lod_items,
            in_view,
        } = {
            let (meshes, custom_meshes) = (&self.meshes, &self.custom_meshes);
            plan_draws(
                list,
                &draws,
                &uses,
                &camera_view,
                |mesh, skin| item_bounds(meshes, custom_meshes, mesh, skin),
                self.frozen_view.is_some().then_some(&mut seen),
            )
        };
        self.culling_bounds = seen;
        cpu.lap(CpuSection::Plan);
        // Shadow maps cost memory, so one exists only while shadows are on,
        // at the size the graphics settings give.
        let map_size = shadows_drawn(list.shadows, &self.graphics).map(|(_, size)| size);
        // Each shadow view's casters are copied after the scene's instances:
        // the sun's cascades, each a box reaching back toward the sun, then
        // the spot lights' views, each near enough to its beam, then each
        // point light's faces, each within its reach.
        let cascades = map_size.map_or(0, |size| size.cascades as usize);
        let mut shadow_view_list: Vec<ShadowView> = frame_uniform.cascades[..cascades]
            .iter()
            .zip(cascade_drawn)
            .map(|(matrix, drawn)| {
                if drawn {
                    ShadowView::Sun(Frustum::of(glam::Mat4::from_cols_array_2d(matrix)))
                } else {
                    ShadowView::Skipped
                }
            })
            .collect();
        for spot in &planned_lights.spot_shadows {
            shadow_view_list.push(ShadowView::Lamp(spot.bounds, Frustum::of(spot.view_proj)));
        }
        for point in &planned_lights.point_shadows {
            for face in point.faces {
                shadow_view_list.push(ShadowView::Lamp(point.bounds, Frustum::of(face)));
            }
        }
        self.casters_planned = casters.iter().map(Vec::len).sum();
        // Swaying casters move with the clock while the wind blows.
        let windy = glam::Vec2::new(list.wind.x, list.wind.z).length() >= 1e-3;
        // The farthest cascade draws casters at the level of detail their
        // size in its map calls for (`GraphicsSettings::shadow_lod`).
        let (meshes, custom_meshes) = (&self.meshes, &self.custom_meshes);
        let uploaded = |mesh: MeshKey| gpu_mesh_in(meshes, custom_meshes, mesh).is_some();
        let by_map_size = map_size
            .filter(|size| self.graphics.shadow_lod && size.cascades > 1)
            .map(|size| FarCascadeLods {
                view: cascades - 1,
                extent: frame_uniform.cascade_texels[cascades - 1] * size.size as f32,
                uploaded: &uploaded,
            });
        let mut view_casters = casters_in_views(
            &casters,
            &shadow_view_list,
            self.cut_out_cascades,
            windy,
            by_map_size,
        );
        cpu.lap(CpuSection::Casters);
        // Water reflecting the screen traces rays too, with the GPU (LE5).
        let water_rays = water_mode.traces_rays(&self.graphics);
        let traces_rays = self.traces_rays(
            ray_shadow_uniform.is_some()
                || ray_ao_uniform.is_some()
                || ray_gi_batch.is_some()
                || ray_reflection_uniform.is_some()
                || water_rays,
        );
        // Every animated item drawn in a view, or traced, is posed once
        // (CO8): its instances and casters get their posed corners.
        let rays = traces_rays.then(|| {
            (
                glam::Vec3::from_slice(&frame_uniform.camera_position[..3]),
                self.graphics.ray_tracing_distance,
            )
        });
        self.pose_frame(
            &draws,
            (&mut instances, &mut view_casters),
            (&mut joints, &skins, &posed),
            rays,
        );
        let mut posed_remade = self.posing.generation != self.posing_generation;
        if posed_remade {
            self.posing_generation = self.posing.generation;
            self.temporal
                .bind_previous(&self.device, self.posing.previous());
        }
        // Plants with packed data per corner (foliage W3b): each instance
        // and caster drawn with one gets its corners' place in the table.
        {
            let (meshes, custom_meshes) = (&self.meshes, &self.custom_meshes);
            self.foliage.mark(
                &self.device,
                (&batches, &mut instances),
                &mut view_casters,
                |key| gpu_mesh_in(meshes, custom_meshes, key),
            );
        }
        if self.foliage.generation != self.foliage_generation {
            self.foliage_generation = self.foliage.generation;
            posed_remade = true;
        }
        cpu.lap(CpuSection::Posing);
        // What moved since last frame, for the motion vectors.
        let moved = self.frame_targets.temporal.as_ref().map(|_| {
            let (posing, foliage) = (&self.posing, &self.foliage);
            let moved = self.history.moved(
                &draws,
                Some(&in_view),
                (
                    |skin, mesh| posing.offset(skin, mesh),
                    |mesh| foliage.first(mesh),
                ),
                clock,
            );
            let blur = list
                .lens
                .motion_blur
                .filter(|share| share.is_finite())
                .map_or(0.0, |share| share.clamp(0.0, 1.0));
            self.temporal.prepare(
                (&self.device, &self.queue),
                &self.history,
                (view_proj, drawn_view_proj, jitter),
                (
                    self.frame_targets.size,
                    taa_upscales.then_some(self.frame_targets.output),
                ),
                (blur, self.graphics.motion_blur),
                clock,
                &moved,
            );
            moved
        });
        cpu.lap(CpuSection::Motion);

        let mut instance_count = instances.iter().map(Vec::len).sum::<usize>() as u32;
        let mut pieces: Vec<&[InstanceRaw]> = instances.iter().map(Vec::as_slice).collect();
        let mut casters_by_view = Vec::with_capacity(view_casters.len());
        for view in &view_casters {
            let groups: Vec<_> = view
                .groups
                .iter()
                .map(|(group, range)| {
                    (
                        *group,
                        range.start + instance_count..range.end + instance_count,
                    )
                })
                .collect();
            casters_by_view.push((groups, view.left_out, view.still_groups));
            pieces.extend(view.instances.iter().map(Vec::as_slice));
            instance_count += view.instances.iter().map(Vec::len).sum::<usize>() as u32;
        }
        self.ensure_instance_capacity(instance_count as usize);
        write_pieces(&self.queue, &self.instance_buffer, &pieces);
        cpu.lap(CpuSection::Instances);
        for font in &list.fonts {
            if self.loaded_fonts.insert(font.path.clone()) {
                self.font_system
                    .db_mut()
                    .load_font_data(font.data.as_ref().clone());
                // A new face can change what fits.
                self.fitted_sizes.clear();
            }
        }
        // UI is drawn in its list's order, layer by layer (see `ui_layers`).
        let layer_ranges = ui_layers(&list.ui);
        let (text_buffers, text_lines, layers_with_text) =
            self.prepare_ui_text(&list.ui, &layer_ranges, viewport_size)?;
        let turned = self.prepare_turned_text(&list.ui, viewport_size)?;
        // The sheet's underlines go first in the buffer.
        let mut ui_vertices = turned.lines.clone();
        let sheet_lines = 0..ui_vertices.len() as u32;
        let mut ui_draws = Vec::with_capacity(layer_ranges.len());
        for (index, range) in layer_ranges.iter().enumerate() {
            let start = ui_vertices.len() as u32;
            ui_vertices.extend(super::ui_vertices(
                &list.ui,
                range.clone(),
                &text_lines,
                viewport_size,
            ));
            let rectangles = start..ui_vertices.len() as u32;
            let images = self.ui_image_draws(
                &list.ui,
                range.clone(),
                &turned.quads,
                viewport_size,
                &mut ui_vertices,
            );
            ui_draws.push(UiLayer {
                rectangles,
                images,
                text: layers_with_text[index],
            });
        }
        self.ensure_ui_capacity(ui_vertices.len());
        if !ui_vertices.is_empty() {
            self.queue
                .write_buffer(&self.ui_buffer, 0, bytemuck::cast_slice(&ui_vertices));
        }
        let (sprite_draws, sprites_culled, emitter_textures) = {
            let textures = &self.textures;
            self.sprites.prepare(
                &self.device,
                &self.queue,
                list,
                (
                    drawn_view_proj,
                    frame_uniform.fog,
                    super::sprites::scene_light(list, self.environment.light_from_above()),
                ),
                &camera_view,
                |key| {
                    textures
                        .get(&key)
                        .and_then(|cached| cached.gpu.as_ref())
                        .map(|gpu| &gpu.srgb)
                },
            )
        };
        let (decal_draws, decals_left_out) = {
            let textures = &self.textures;
            let normals = self
                .frame_targets
                .occlusion
                .as_ref()
                .map(|targets| &targets.normal_view);
            self.decal_pass.prepare(
                (&self.device, &self.queue),
                list,
                (self.graphics.decals, &camera_view),
                normals,
                |key| {
                    textures
                        .get(&key)
                        .and_then(|cached| cached.gpu.as_ref())
                        .map(|gpu| &gpu.srgb)
                },
            )
        };
        // GPU particles, where compute shaders run.
        if list.particle_emitters.is_empty() || !self.capabilities.compute_shaders {
            self.gpu_particles = None;
        } else {
            self.gpu_particles
                .get_or_insert_with(|| Box::new(gpu_particles::GpuParticles::new(&self.device)))
                .prepare(
                    (&self.device, &self.queue),
                    list,
                    &emitter_textures,
                    gpu_particles::PrepassView {
                        view_proj: glam::Mat4::from_cols_array_2d(&frame_uniform.view_proj),
                        inv_view_proj: glam::Mat4::from_cols_array_2d(&frame_uniform.inv_view_proj),
                        view: glam::Mat4::from_cols_array_2d(&frame_uniform.view),
                        size: self.frame_targets.size,
                    },
                    self.frame_targets
                        .occlusion
                        .as_ref()
                        .map(|targets| (&targets.depth_view, &targets.normal_view)),
                );
        }

        // The spot and point lights' shadow maps exist only while one of
        // them casts shadows.
        let spot_map = quality
            .spot_map()
            .filter(|_| !planned_lights.spot_shadows.is_empty());
        let point_map = quality
            .point_map()
            .filter(|_| !planned_lights.point_shadows.is_empty());
        let mut changed = false;
        if self.shadow_map.as_ref().map(|map| map.size) != map_size {
            self.shadow_map = map_size.map(|size| {
                // The shader holds at most `SHADOW_CASCADES` cascades.
                debug_assert!(
                    size.size <= SHADOW_MAP_SIZE && size.cascades as usize <= SHADOW_CASCADES
                );
                let (texture, array, layers) = create_shadow_layers(
                    &self.device,
                    "shadow map",
                    (size.size, size.cascades),
                    false,
                );
                ShadowMap {
                    size,
                    _alive: self.shadow_maps_alive.clone(),
                    bytes: target_bytes((size.size, size.size, size.cascades), DEPTH_FORMAT, 1),
                    drawn_from: vec![None; layers.len()],
                    kept: None,
                    texture,
                    array,
                    layers,
                }
            });
            changed = true;
        }
        if self.spot_shadow_map.as_ref().map(|map| map.size) != spot_map {
            self.spot_shadow_map = spot_map.map(|size| {
                debug_assert!(size.lights as usize <= MAX_SPOT_SHADOWS);
                let (texture, array, layers) = create_shadow_layers(
                    &self.device,
                    "spot shadow map",
                    (size.size, size.lights),
                    true,
                );
                ShadowMap {
                    size,
                    _alive: self.spot_shadow_maps_alive.clone(),
                    bytes: target_bytes((size.size, size.size, size.lights), DEPTH_FORMAT, 1),
                    drawn_from: vec![None; layers.len()],
                    kept: None,
                    texture,
                    array,
                    layers,
                }
            });
            changed = true;
        }
        if self.point_shadow_map.as_ref().map(|map| map.size) != point_map {
            self.point_shadow_map = point_map.map(|size| {
                debug_assert!(size.lights as usize <= MAX_POINT_SHADOWS);
                let count = size.lights * POINT_SHADOW_FACES;
                let (texture, array, layers) = create_shadow_layers(
                    &self.device,
                    "point shadow map",
                    (size.size, count),
                    true,
                );
                ShadowMap {
                    size,
                    _alive: self.point_shadow_maps_alive.clone(),
                    bytes: target_bytes((size.size, size.size, count), DEPTH_FORMAT, 1),
                    drawn_from: vec![None; layers.len()],
                    kept: None,
                    texture,
                    array,
                    layers,
                }
            });
            changed = true;
        }
        if changed || posed_remade {
            self.bind_frame();
        }

        // The frame's commands, in parts that are encoded side by side
        // (renderer/encoders.rs): a part ends before each run of passes
        // with many draw calls.
        let mut encoder = encoders::FrameEncoders::new(&self.device, self.frame_in_parts);
        // Times this frame unless the last measurement is still being read.
        let timed = match &mut self.gpu_timer {
            Some(timer) if self.time_gpu => timer.collect(&self.device),
            _ => false,
        };
        let timer = self.gpu_timer.as_ref().filter(|_| timed);
        let mark = |encoder: &mut wgpu::CommandEncoder, pass| {
            if let Some(timer) = timer {
                timer.mark(encoder, pass);
            }
        };
        // A stamp between a pass's draws, where what is drawn changes.
        let stamp_part = |pass: &mut wgpu::RenderPass<'_>, within, started| {
            if let Some(timer) = timer {
                timer.part(pass, within, started);
            }
        };
        // First, every animated item morphed and skinned once (CO8), and
        // plants newly drawn copied into the foliage table.
        mark(&mut encoder, FramePass::CharacterPosing);
        let posed_any = {
            let (meshes, custom_meshes) = (&self.meshes, &self.custom_meshes);
            self.foliage
                .record(&mut encoder, |key| gpu_mesh_in(meshes, custom_meshes, key));
            self.posing
                .record(&mut encoder, |key| gpu_mesh_in(meshes, custom_meshes, key))
        };
        mark(&mut encoder, FramePass::AccelerationStructures);
        let path_traced = self.path_traced();
        cpu.lap(CpuSection::Interface);
        let may_trace = self.may_trace_rays();
        let tracing = ray_tracing::prepare_scene(
            (&mut self.ray_scene, &mut self.parked_ray_scene, may_trace),
            (
                (&self.device, &self.queue),
                &self.meshes,
                &self.custom_meshes,
            ),
            &mut encoder,
            (
                traces_rays,
                self.frame,
                (
                    glam::Vec3::from_slice(&frame_uniform.camera_position[..3]),
                    ray_sway::Sight::of(view_proj, self.frame_targets.size.1),
                ),
                self.graphics.ray_tracing_distance,
                ray_gi_batch.is_some()
                    || water_rays
                    || path_traced
                    || ray_reflection_uniform.is_some(),
                frame_uniform.clock,
            ),
            (&draws, &groups, &self.posing),
        );
        cpu.lap(CpuSection::RayScene);
        // Without ray queries, GI's rays walk a scene built in software.
        if ray_gi_batch.is_some() && self.graphics.ray_traced_gi_in_software {
            let soft = self
                .soft_scene
                .get_or_insert_with(|| Box::new(ray_soft::SoftScene::new(&self.device)));
            soft.prepare(
                (&self.device, &self.queue),
                (
                    self.frame,
                    glam::Vec3::from_slice(&frame_uniform.camera_position[..3]),
                    self.graphics.ray_tracing_distance,
                ),
                &draws,
                |key| gpu_mesh_in(&self.meshes, &self.custom_meshes, key),
            );
        } else {
            self.soft_scene = None;
        }
        if let Some(scene) = &mut self.ray_scene {
            let [on, _, reach, _] = frame_uniform.ray_shadows;
            scene.stats.shadow_reach = if on > 0.5 { reach.round() as u32 } else { 0 };
        }
        let water_scene = self
            .ray_scene
            .as_deref()
            .filter(|_| water_rays)
            .and_then(|rays| {
                let surfaces = rays.surfaces.as_ref()?;
                Some((
                    rays.tlas(),
                    &surfaces.buffer,
                    self.graphics.ray_tracing_distance,
                ))
            });
        self.water.trace(
            (&self.device, &self.queue),
            [
                &self.frame_layout,
                &self.material_layout,
                &self.skinning.layout,
            ],
            water_scene,
        );
        mark(&mut encoder, FramePass::Shadows);
        let mut counts = FrameCounts {
            items_submitted,
            items_culled: culled + past_cull_distance,
            shadow_casters_culled: casters_by_view.iter().map(|(_, culled, _)| culled).sum(),
            sprites_culled,
            decals_drawn: list.decals.len() as u32 - decals_left_out,
            decals_left_out,
            lod_items,
            ..FrameCounts::default()
        };
        // While snow with a thickness lies, the passes that lift corners
        // under it draw each mesh's skirt too (`snow_caps.rs`).
        let snow_caps = list.weather.snow > 0.0 && list.weather.snow_thickness > 0.0;
        let mut shadow_casters_drawn = 0;
        let mut count = |mesh: &GpuMesh, instances: &Range<u32>| {
            let instances = u64::from(instances.end - instances.start);
            counts.draw_calls += 1;
            counts.triangles += u64::from(mesh.index_count / 3) * instances;
            counts.vertices += u64::from(mesh.vertex_count) * instances;
        };

        // One pass per cascade, each drawing into its own layer of the map,
        // then one per shadow-casting spot light and one per face of each
        // shadow-casting point light, each drawing the casters that can
        // reach its view. Each view's matrix is at its index in
        // `cascade_buffer`: the spot lights' after every cascade's, the
        // point lights' after every spot light's.
        let (cascade_casters, lamp_casters) = casters_by_view.split_at(cascades);
        let (spot_casters, point_casters) =
            lamp_casters.split_at(planned_lights.spot_shadows.len());
        // Each lamp view keeps its layer, draws it, or draws its moving
        // casters over its kept still ones (renderer/lamp_shadows.rs).
        let lamp_views = &view_casters[cascades..];
        let lamp_matrices = (planned_lights.spot_shadows.iter())
            .map(|spot| spot.view_proj)
            .chain(
                planned_lights
                    .point_shadows
                    .iter()
                    .flat_map(|point| point.faces),
            );
        let prints: Vec<Option<u64>> = lamp_views
            .iter()
            .zip(lamp_matrices)
            .map(|(view, matrix)| {
                (!lamp_shadows::redrawn_every_frame()).then(|| {
                    lamp_shadows::fingerprint(
                        matrix,
                        view,
                        |mesh| self.gpu_mesh(mesh).map(|mesh| mesh.version),
                        |material| self.cut_out_material(&uses, material).serial,
                    )
                })
            })
            .collect();
        let moving: Vec<bool> = lamp_views
            .iter()
            .map(|view| view.still_groups < view.groups.len())
            .collect();
        let spot_count = planned_lights.spot_shadows.len();
        let spot_plans = lamp_shadows::plan_layers(
            &self.device,
            self.spot_shadow_map.as_mut(),
            &prints[..spot_count],
            &moving[..spot_count],
        );
        let point_plans = lamp_shadows::plan_layers(
            &self.device,
            self.point_shadow_map.as_mut(),
            &prints[spot_count..],
            &moving[spot_count..],
        );
        // Views whose still casters weren't drawn again.
        let lamp_views_kept = (spot_plans.iter().chain(&point_plans))
            .filter(|plan| matches!(plan, LampDraw::Kept | LampDraw::Over { still: false }))
            .count();
        let mut lamp_shadow_draws = 0;
        // Each view: its index in `cascade_buffer`, what it does, its map's
        // texture, kept still casters and layer, and its casters.
        let cascades = self.shadow_map.iter().flat_map(|map| {
            let layers = map.layers.iter().zip(cascade_casters).enumerate();
            layers.map(move |(index, view)| {
                (
                    index,
                    LampDraw::All,
                    (&map.texture, map.kept.as_ref(), index),
                    view,
                )
            })
        });
        let spots = self.spot_shadow_map.iter().flat_map(|map| {
            let layers = map.layers.iter().zip(spot_casters).enumerate();
            layers.map(|(index, view)| {
                (
                    SHADOW_CASCADES + index,
                    spot_plans[index],
                    (&map.texture, map.kept.as_ref(), index),
                    view,
                )
            })
        });
        let points = self.point_shadow_map.iter().flat_map(|map| {
            let layers = map.layers.iter().zip(point_casters).enumerate();
            layers.map(|(index, view)| {
                let at = SHADOW_CASCADES + MAX_SPOT_SHADOWS + index;
                (
                    at,
                    point_plans[index],
                    (&map.texture, map.kept.as_ref(), index),
                    view,
                )
            })
        });
        let mut cascades_timed = [false; SHADOW_CASCADES];
        let mut cascade_counts = [(0u32, 0u64, 0u32); SHADOW_CASCADES];
        let mut after_cascades = false;
        for (index, plan, (texture, kept, at), (layer, (casters, _, still))) in
            cascades.chain(spots).chain(points)
        {
            // A cascade waiting its turn keeps what it drew: not even cleared.
            if index < SHADOW_CASCADES && !cascade_drawn[index] {
                continue;
            }
            if let Some(timer) = timer {
                if index < SHADOW_CASCADES {
                    timer.mark_cascade(&mut encoder, index);
                    cascades_timed[index] = true;
                } else if !after_cascades {
                    timer.mark_cascade(&mut encoder, SHADOW_CASCADES);
                    after_cascades = true;
                }
            }
            let (still_casters, moving_casters) = casters.split_at(*still);
            // What's drawn into which layer, cleared first or not, with a
            // copy of the kept still casters into the layer between.
            let steps: Vec<ShadowStep> = match (plan, kept) {
                // A lamp's view with nothing changed keeps its layer too.
                (LampDraw::Kept, _) => continue,
                (LampDraw::Still, _) => vec![ShadowStep::Draw(layer, true, still_casters)],
                (LampDraw::Over { still }, Some(kept)) => {
                    let mut steps = Vec::with_capacity(3);
                    if still {
                        steps.push(ShadowStep::Draw(&kept.layers[at], true, still_casters));
                    }
                    steps.push(ShadowStep::Copy);
                    steps.push(ShadowStep::Draw(layer, false, moving_casters));
                    steps
                }
                _ => vec![ShadowStep::Draw(layer, true, casters)],
            };
            let (mut view_casters, mut view_triangles, mut view_draws) = (0u32, 0u64, 0u32);
            // Each view with enough casters is a part of its own.
            if casters.len() >= encoders::WORTH_A_PART {
                encoder.part("shadow view");
            }
            for step in steps {
                let (target, clear, groups) = match step {
                    ShadowStep::Draw(target, clear, groups) => (target, clear, groups),
                    ShadowStep::Copy => {
                        let Some(kept) = kept else { continue };
                        let layer_of = |texture| wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: 0,
                                y: 0,
                                z: at as u32,
                            },
                            aspect: wgpu::TextureAspect::All,
                        };
                        let size = texture.size();
                        encoder.copy_texture_to_texture(
                            layer_of(&kept.texture),
                            layer_of(texture),
                            wgpu::Extent3d {
                                depth_or_array_layers: 1,
                                ..size
                            },
                        );
                        continue;
                    }
                };
                let load = if clear {
                    wgpu::LoadOp::Clear(1.0)
                } else {
                    wgpu::LoadOp::Load
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow view"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: target,
                        depth_ops: Some(wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&self.shadow_pipeline);
                let offset = (index as u64 * CASCADE_UNIFORM_STRIDE) as u32;
                pass.set_bind_group(0, &self.shadow_bind_group, &[offset]);
                pass.set_bind_group(1, &self.skinning.empty, &[]);
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                // Which caster group's pipeline and material are bound: a
                // cut-out's are its own, every other caster's the plain ones.
                let mut bound: Option<u32> = None;
                // See-through items, and parts with `CastShadow` off, cast
                // no shadow. The groups come whole casters first, then
                // cut-outs by material (`casters_in_views`), so the pipeline
                // changes once.
                for ((mesh, cut_out), instances) in groups {
                    let Some(mesh) = self.gpu_mesh(*mesh) else {
                        continue;
                    };
                    if *cut_out != bound {
                        match cut_out {
                            Some(material) => {
                                let gpu = self.cut_out_material(&uses, *material);
                                pass.set_pipeline(&self.shadow_cut_out_pipeline);
                                pass.set_bind_group(1, &gpu.bind_group, &[]);
                            }
                            None => {
                                pass.set_pipeline(&self.shadow_pipeline);
                                pass.set_bind_group(1, &self.skinning.empty, &[]);
                            }
                        }
                        bound = *cut_out;
                    }
                    pass.set_bind_group(2, self.skin_group(mesh), &[]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), mesh.index_format);
                    count(mesh, instances);
                    shadow_casters_drawn += instances.end - instances.start;
                    view_casters += instances.end - instances.start;
                    view_triangles += u64::from(mesh.index_count / 3)
                        * u64::from(instances.end - instances.start);
                    view_draws += 1;
                    pass.draw_indexed(0..mesh.index_count, 0, instances.clone());
                }
            }
            if index < SHADOW_CASCADES {
                cascade_counts[index] = (view_casters, view_triangles, view_draws);
            } else {
                lamp_shadow_draws += view_draws;
            }
        }
        if let Some(timer) = timer
            && !after_cascades
        {
            timer.mark_cascade(&mut encoder, SHADOW_CASCADES);
        }
        let frame_bind_group = &self.frame_bind_group;
        encoder.part("prepass");
        mark(&mut encoder, FramePass::Prepass);
        let mut motion_draws = 0;
        let mut prepass_draws = 0;

        // While ambient occlusion or contact shadows are on: solid items'
        // depth and normals, then (for ambient occlusion) how hidden each
        // pixel is by what's around it.
        if let Some(occlusion) = &self.frame_targets.occlusion {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("depth and normal prepass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        depth_slice: None,
                        view: &occlusion.normal_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &occlusion.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(0, &self.prepass_frame_bind_group, &[]);
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                let mut drawing = None;
                for batch in batches.iter().filter(|batch| !batch.transparent) {
                    if drawing != Some(batch.part) {
                        drawing = Some(batch.part);
                        stamp_part(&mut pass, PartPass::Prepass, drawing);
                    }
                    let Some(mesh) = self.bind_batch(&mut pass, batch, BatchPass::Prepass) else {
                        continue;
                    };
                    count(mesh, &batch.instances);
                    prepass_draws += 1;
                    pass.draw_indexed(0..mesh.drawn_indices(snow_caps), 0, batch.instances.clone());
                }
                stamp_part(&mut pass, PartPass::Prepass, None);
            }
            // The motion vectors, from what the prepass drew.
            if let (Some(targets), Some(moved)) = (&self.frame_targets.temporal, &moved) {
                motion_draws = self.temporal.draw_motion(
                    &mut encoder,
                    (targets, &self.history),
                    (
                        &occlusion.depth_view,
                        &self.prepass_frame_bind_group,
                        &self.skinning.empty,
                    ),
                    moved,
                    |pass, key| {
                        let mesh = self.gpu_mesh(key)?;
                        pass.set_bind_group(2, self.skin_group(mesh), &[]);
                        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                        pass.set_index_buffer(mesh.indices.slice(..), mesh.index_format);
                        Some(mesh.index_count)
                    },
                );
            }
            mark(&mut encoder, FramePass::AmbientOcclusion);
            if screen_space_occlusion {
                self.occlusion.draw_raw(&mut encoder, occlusion);
            }
            if let (Some(pass), Some(uniform), Some(rays)) =
                (&self.ray_ao, &ray_ao_uniform, &self.ray_scene)
            {
                pass.draw(
                    (&self.device, &self.queue),
                    &mut encoder,
                    (rays, occlusion),
                    uniform,
                    screen_space_occlusion,
                );
            }
            if ambient_occlusion {
                self.occlusion.blur(&mut encoder, occlusion);
            }
        } else {
            mark(&mut encoder, FramePass::AmbientOcclusion);
        }
        mark(&mut encoder, FramePass::RayTracedShadows);
        let ray_shadows_drawn = if let (Some(shadows), Some(uniform), Some(rays), Some(prepass)) = (
            &self.ray_shadows,
            &ray_shadow_uniform,
            &self.ray_scene,
            &self.frame_targets.occlusion,
        ) {
            shadows.draw(
                (&self.device, &self.queue),
                &mut encoder,
                (rays, prepass),
                (&self.light_buffer, &self.tile_buffer),
                uniform,
            );
            true
        } else {
            false
        };
        mark(&mut encoder, FramePass::RayTracedGi);
        // The camera's probes move along with it first.
        self.light_probes.follow_camera(&mut encoder);
        let gi_scene = match (&self.ray_scene, &self.soft_scene) {
            (Some(rays), _) => rays.surfaces.as_ref().map(|surfaces| {
                ray_gi::GiScene::Hardware(rays.tlas(), &surfaces.buffer, &rays.cut_outs)
            }),
            (None, Some(soft)) => Some(ray_gi::GiScene::Software(
                soft.buffer(),
                &soft.surfaces.buffer,
            )),
            (None, None) => None,
        };
        let ray_gi_drawn = if let (Some(gi), Some(batch), Some(scene)) =
            (&mut self.ray_gi, &ray_gi_batch, gi_scene)
        {
            gi.trace(
                (&self.device, &self.queue),
                &mut encoder,
                scene,
                batch,
                &self.light_probes,
            )
        } else {
            false
        };
        mark(&mut encoder, FramePass::RayTracedReflections);
        let ray_reflections_drawn = match (
            &self.ray_reflections,
            &ray_reflection_uniform,
            &self.ray_scene,
            &self.frame_targets.occlusion,
        ) {
            (Some(pass), Some(uniform), Some(rays), Some(prepass)) => {
                rays.surfaces.as_ref().is_some_and(|surfaces| {
                    pass.trace(
                        (&self.device, &self.queue),
                        &mut encoder,
                        (rays, &surfaces.buffer, prepass),
                        uniform,
                    )
                })
            }
            _ => false,
        };
        if ray_reflection_uniform.is_some() && !ray_reflections_drawn {
            // Nothing traced this frame: the reflection probes stay.
            self.queue.write_buffer(
                &self.frame_buffer,
                std::mem::offset_of!(FrameUniform, ray_reflections) as u64,
                bytemuck::bytes_of(&[0.0f32; 4]),
            );
        }
        mark(&mut encoder, FramePass::ScreenReflections);
        let screen_reflections_drawn = match (
            &self.screen_reflections,
            &screen_reflection_uniform,
            &self.frame_targets.occlusion,
        ) {
            (Some(pass), Some(uniform), Some(prepass)) => {
                pass.march(
                    (&self.device, &self.queue),
                    &mut encoder,
                    (prepass, &self.frame_targets.hdr_view),
                    uniform,
                );
                true
            }
            // No last frame to read: nothing is found on the screen.
            (Some(pass), None, _) => {
                encoder.clear_buffer(&pass.buffer, 0, None);
                false
            }
            _ => false,
        };
        mark(&mut encoder, FramePass::VolumetricFog);
        if let Some(fog) = &self.volumetric_fog {
            fog.scatter(&mut encoder);
        }
        mark(&mut encoder, FramePass::Particles);
        if let Some(particles) = &self.gpu_particles {
            particles.run(&mut encoder);
        }
        // The sky's clouds, for the scene's sky pass to lay over its sky.
        mark(&mut encoder, FramePass::Clouds);
        self.cloud_trace.draw(&mut encoder);
        encoder.part("scene");
        mark(&mut encoder, FramePass::Scene);
        let emitter_draws = self
            .gpu_particles
            .as_ref()
            .map_or_else(Vec::new, |particles| particles.draws());

        // Where the prepass drew the same solid items with as many samples
        // a pixel, the scene's depth starts a step behind the prepass's in
        // place of the far plane (renderer/depth_start.rs): what's hidden
        // then fails the depth test before it is lit, and what shows is
        // drawn exactly as it would be.
        let depth_started = match &self.frame_targets.occlusion {
            Some(prepass) if self.frame_targets.samples == 1 => self.depth_start.draw(
                &self.device,
                &mut encoder,
                (&prepass.depth_view, &prepass.normal_view),
                &self.frame_targets.depth_view,
            ),
            _ => false,
        };
        // The scene is drawn into the HDR texture; post-processing then brings
        // it to `view`. With MSAA it's drawn into the multisampled texture,
        // whose samples are averaged into the HDR one as the pass ends.
        // Solid items come first, then the sky only where none was drawn
        // (its depth test), so the sky and its clouds are worked out only
        // for the pixels that show them. Solid items' outlines come just
        // after them, and screen-space outlines after the sky. Then
        // see-through items over all that:
        // water first, then the rest, then sprites. While water sees
        // through itself, the pass stops before the see-through items so
        // the solid scene can be copied for it (renderer/water.rs), and a
        // second pass carries on where it left off.
        let see_through = batches
            .iter()
            .position(|batch| batch.transparent)
            .unwrap_or(batches.len());
        let (solid, see_through) = batches.split_at(see_through);
        let copied = self.water.copy.is_some() && see_through.iter().any(|batch| batch.water);
        let water_group = self
            .water_frame_bind_group
            .as_ref()
            .filter(|_| copied)
            .unwrap_or(frame_bind_group);
        let mut drawn = (0, 0);
        // The depth fill's draw calls, triangles and corners.
        let mut depth_fill = (0, 0, 0);
        let mut draw_batches = |pass: &mut wgpu::RenderPass<'_>, batches: &[Batch], solid: bool| {
            let mut water_bound = false;
            let mut drawing = None;
            for batch in batches {
                if solid && drawing != Some(batch.part) {
                    drawing = Some(batch.part);
                    stamp_part(pass, PartPass::Scene, drawing);
                }
                if batch.water != water_bound {
                    water_bound = batch.water;
                    let group = if water_bound {
                        water_group
                    } else {
                        frame_bind_group
                    };
                    pass.set_bind_group(0, group, &[]);
                }
                let Some(mesh) = self.bind_batch(pass, batch, BatchPass::Scene) else {
                    continue;
                };
                count(mesh, &batch.instances);
                drawn.0 += batch.instances.end - batch.instances.start;
                drawn.1 += 1;
                pass.draw_indexed(0..mesh.drawn_indices(snow_caps), 0, batch.instances.clone());
            }
            if water_bound {
                pass.set_bind_group(0, frame_bind_group, &[]);
            }
            // Outlines (solid items only) go behind what was just drawn.
            for batch in batches.iter().filter(|batch| batch.outline) {
                let Some(mesh) = self.bind_batch(pass, batch, BatchPass::Scene) else {
                    continue;
                };
                pass.set_pipeline(&self.outline_pipeline);
                count(mesh, &batch.instances);
                pass.draw_indexed(0..mesh.index_count, 0, batch.instances.clone());
            }
        };
        let edges = self
            .frame_targets
            .occlusion
            .as_ref()
            .filter(|_| list.edges.is_some());
        if let Some(grid) = &list.grid {
            self.grid_pass.prepare(
                &self.queue,
                frame_uniform.view_proj,
                frame_uniform.camera_position,
                grid,
            );
        }
        // Each pass is a block so it ends (is dropped) before the next
        // begins and before `encoder.finish()`, which need the encoder back.
        {
            let mut pass = begin_scene_pass(
                &mut encoder,
                &self.frame_targets,
                (false, depth_started),
                copied,
            );
            pass.set_bind_group(0, frame_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            // The cut-out surfaces' depth first, so they're lit only once
            // per pixel, and what's behind them not at all. Where the depth
            // started from the prepass's it's there already: a cut-out's
            // pixel kept the prepass's depth exactly (renderer/depth_start.rs).
            let fill = !depth_started || cut_out_fill_forced();
            if fill && solid.iter().any(|batch| batch.prefilled) {
                stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::CutOutDepth));
            }
            for batch in solid.iter().filter(|batch| fill && batch.prefilled) {
                let Some(mesh) = self.bind_batch(&mut pass, batch, BatchPass::DepthFill) else {
                    continue;
                };
                let instances = u64::from(batch.instances.end - batch.instances.start);
                depth_fill.0 += 1;
                depth_fill.1 += u64::from(mesh.index_count / 3) * instances;
                depth_fill.2 += u64::from(mesh.vertex_count) * instances;
                pass.draw_indexed(0..mesh.drawn_indices(snow_caps), 0, batch.instances.clone());
            }
            draw_batches(&mut pass, solid, true);
            stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::Sky));
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(2, self.cloud_trace.sky_group(), &[]);
            pass.draw(0..3, 0..1);
            stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::Decals));
            // Projected decals over the solid scene (the sky's pixels are
            // left alone by the prepass's depth).
            self.decal_pass.draw(&mut pass, &decal_draws);
            if list.grid.is_some() {
                self.grid_pass.draw(&mut pass);
                pass.set_bind_group(0, frame_bind_group, &[]);
            }
            // Screen-space outlines over the solid scene and the sky.
            if let Some(prepass) = edges {
                pass.set_pipeline(&self.edge_pipeline);
                pass.set_bind_group(0, &prepass.edge_group, &[]);
                pass.draw(0..3, 0..1);
                pass.set_bind_group(0, frame_bind_group, &[]);
            }
            if !copied {
                stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::SeeThrough));
                draw_batches(&mut pass, see_through, false);
                // Last, over everything, but hidden behind solid items.
                stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::Sprites));
                self.sprites.draw(&mut pass, &sprite_draws, &emitter_draws);
            }
            stamp_part(&mut pass, PartPass::Scene, None);
        }
        if copied {
            let targets = &self.frame_targets;
            self.water.copy_scene(
                (&self.device, &self.queue),
                &mut encoder,
                (&targets.hdr_view, &targets.depth_view),
                water::CopyView {
                    inv_view_proj: frame_uniform.inv_view_proj,
                    camera_position: frame_uniform.camera_position,
                },
            );
            let mut pass = begin_scene_pass(&mut encoder, targets, (true, true), false);
            pass.set_bind_group(0, frame_bind_group, &[]);
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::SeeThrough));
            draw_batches(&mut pass, see_through, false);
            stamp_part(&mut pass, PartPass::Scene, Some(DrawnPart::Sprites));
            self.sprites.draw(&mut pass, &sprite_draws, &emitter_draws);
            stamp_part(&mut pass, PartPass::Scene, None);
        }
        (counts.items_drawn, counts.batches) = drawn;
        counts.draw_calls += motion_draws + depth_fill.0 + decal_draws.len() as u32;
        counts.prepass_draws = prepass_draws;
        counts.motion_draws = motion_draws;
        counts.depth_fill_draws = depth_fill.0;
        counts.depth_from_prepass = depth_started;
        for batch in solid
            .iter()
            .filter(|batch| batch.part == DrawnPart::Scattered)
        {
            counts.scattered_batches += 1;
            counts.scattered_items += batch.instances.end - batch.instances.start;
        }
        // The light the volumetric fog scattered in front of what was drawn.
        if let (Some(fog), Some(occlusion)) = (&self.volumetric_fog, &self.frame_targets.occlusion)
        {
            fog.composite(
                &self.device,
                &mut encoder,
                frame_bind_group,
                &self.frame_targets.hdr_view,
                &occlusion.depth_view,
            );
        }
        // Sun rays streaming from the sky's gaps, added to the scene.
        // Under TAA their noise turns each frame, so TAA averages it away.
        let taa = self
            .frame_targets
            .temporal
            .as_ref()
            .is_some_and(|temporal| temporal.has_taa());
        let turn = if taa { (self.frame % 64) as f32 } else { 0.0 };
        if let (Some(rays), Some(occlusion)) = (
            sun_rays::uniform(list, &self.graphics, aspect, turn),
            &self.frame_targets.occlusion,
        ) {
            let pass = self
                .sun_rays_pass
                .get_or_insert_with(|| Box::new(sun_rays::SunRaysPass::new(&self.device)));
            pass.draw(
                &self.device,
                &mut encoder,
                &rays,
                &self.frame_targets.hdr_view,
                &occlusion.depth_view,
            );
        }
        mark(&mut encoder, FramePass::PostProcessing);

        // TAA, then motion blur, each copied over the scene, then depth of
        // field. While TAA upscales, it comes last: motion blur and depth of
        // field are drawn on the scene first, and TAA reads the result.
        let targets = &self.frame_targets;
        let temporal_targets = targets.temporal.as_ref().map(|t| (t, &self.history));
        if let Some(temporal) = temporal_targets
            && !taa_upscales
        {
            self.temporal.draw_taa(&mut encoder, temporal, &targets.hdr);
        }
        if let Some(temporal) = temporal_targets {
            self.temporal
                .draw_motion_blur(&mut encoder, temporal, &targets.hdr);
        }
        if let Some(target) = &targets.depth_of_field {
            self.post.depth_of_field.draw(&mut encoder, target);
        }
        if let Some(temporal) = temporal_targets
            && taa_upscales
        {
            self.temporal.draw_taa(&mut encoder, temporal, &targets.hdr);
        }
        self.post.draw(
            &mut encoder,
            &self.frame_targets,
            view,
            post.enabled && post.bloom_enabled && post.bloom_intensity > 0.0,
        );

        // The editor's debug view, over the finished frame and under the
        // UI (renderer/debug_views.rs).
        #[cfg(feature = "debug-views")]
        if let Some(mut debug) = self.debug_views.take() {
            // The sun, the sky and the lamps, for the Path Traced view.
            let lighting = path_tracer::lighting_key(&[
                bytemuck::bytes_of(&frame_uniform.sun_direction),
                bytemuck::bytes_of(&frame_uniform.sun_radiance),
                bytemuck::bytes_of(&frame_uniform.sky_top),
                bytemuck::bytes_of(&frame_uniform.sky_horizon),
                bytemuck::bytes_of(&frame_uniform.sky_ground),
                bytemuck::bytes_of(&frame_uniform.environment),
                bytemuck::bytes_of(&frame_uniform.night),
                bytemuck::bytes_of(&frame_uniform.lights),
                bytemuck::cast_slice(&planned_lights.lights),
            ]);
            debug.draw(
                self,
                &mut encoder,
                view,
                (&batches, &draws),
                (view_proj, lighting),
            );
            self.debug_views = Some(debug);
        }

        mark(&mut encoder, FramePass::Ui);
        if turned.drawn
            && let Some((sheet, _, _)) = &self.turned_text.sheet
        {
            let sheet_view = sheet.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("turned UI text"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: &sheet_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            if !sheet_lines.is_empty() {
                pass.set_pipeline(&self.ui_pipeline);
                pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
                pass.draw(sheet_lines.clone(), 0..1);
            }
            self.turned_text
                .renderer
                .render(&self.text_atlas, &self.turned_text.viewport, &mut pass)
                .context("could not render turned UI text")?;
        }
        let _ = &turned.buffers;

        let ui_drawn = !ui_vertices.is_empty() || !text_buffers.is_empty();
        if ui_drawn {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("UI overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            for (index, layer) in ui_draws.iter().enumerate() {
                if !layer.rectangles.is_empty() {
                    pass.set_pipeline(&self.ui_pipeline);
                    pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
                    pass.draw(layer.rectangles.clone(), 0..1);
                }
                if !layer.images.is_empty() {
                    pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
                    for (key, start, count) in &layer.images {
                        let (pipeline, group) = if *key == SHEET_KEY {
                            let Some((_, group, _)) = &self.turned_text.sheet else {
                                continue;
                            };
                            (&self.ui_sheet_pipeline, group)
                        } else {
                            let Some(group) = self.ui_image_groups.get(key) else {
                                continue;
                            };
                            (&self.ui_image_pipeline, group)
                        };
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, group, &[]);
                        pass.draw(*start..*start + *count, 0..1);
                    }
                }
                if layer.text {
                    self.text_renderers[index]
                        .render(&self.text_atlas, &self.text_viewport, &mut pass)
                        .context("could not render UI text")?;
                }
            }
        }

        if let Some(timer) = timer {
            timer.end(&mut encoder);
        }
        cpu.lap(CpuSection::Passes);
        let commands = encoder.finish();
        counts.command_buffers = commands.len() as u32;
        cpu.lap(CpuSection::Encode);
        let submitted = self.queue.submit(commands);
        if let Some(latency) = &mut self.latency {
            latency.submitted(&self.queue);
        }
        cpu.lap(CpuSection::Submit);
        self.wait_for_frame_before(submitted);
        cpu.skip();
        if super::occlusion_probe::reads_on(self.frame)
            && let Some(targets) = &self.frame_targets.occlusion
        {
            super::occlusion_probe::read(
                &self.device,
                &self.queue,
                &targets.depth_texture,
                view_proj,
            );
        }
        if timed && let Some(timer) = &mut self.gpu_timer {
            let shadows = self.shadow_map.is_some()
                || self.spot_shadow_map.is_some()
                || self.point_shadow_map.is_some();
            let prepass = self.frame_targets.occlusion.is_some();
            timer.submitted(
                [
                    posed_any,
                    tracing,
                    shadows,
                    prepass,
                    prepass && ambient_occlusion,
                    ray_shadows_drawn,
                    ray_gi_drawn,
                    ray_reflections_drawn,
                    screen_reflections_drawn,
                    self.volumetric_fog.is_some(),
                    self.gpu_particles.is_some(),
                    self.cloud_trace.traced(),
                    true,
                    true,
                    turned.drawn || ui_drawn,
                ],
                cascades_timed,
            );
        }
        counts.shadow_casters_drawn = shadow_casters_drawn;
        counts.cascade_casters = cascade_counts.map(|(casters, _, _)| casters);
        counts.cascade_triangles = cascade_counts.map(|(_, triangles, _)| triangles);
        counts.cascade_draws = cascade_counts.map(|(_, _, draws)| draws);
        counts.lamp_shadow_draws = lamp_shadow_draws;
        counts.lamp_views_kept = lamp_views_kept as u32;
        self.counts = counts;
        match &self.frame_targets.temporal {
            Some(temporal) => {
                // Where each posed item's corners are, for next frame's
                // motion vectors.
                let posed = draws
                    .iter()
                    .filter(|draw| draw.item.id != 0)
                    .filter_map(|draw| {
                        // The skin first: most draws have none, and a
                        // draw's mesh key is read through its item's shape.
                        let skin = draw.item.skin.as_deref()?;
                        let mesh = draw.mesh();
                        let offset = self.posing.offset(skin, mesh)?;
                        Some(((draw.item.id, mesh), offset))
                    })
                    .collect();
                self.history
                    .finish(list, (view_proj, clock), posed, temporal.has_taa());
            }
            None => self.history.reset(),
        }
        self.forget_undrawn();
        self.clusters = clusters;
        cpu.lap(CpuSection::After);
        self.cpu_times = cpu.times();
        Ok(())
    }

    /// Which of the sun's cascades this frame draws. In a heavy scene (more
    /// shadow casters than [`HEAVY_SHADOW_CASTERS`]) the two farthest take
    /// turns, so each frame draws one: they hold most of its shadow
    /// triangles and cover what's far away, where one frame's lag doesn't
    /// show. With four cascades (Ultra) the farthest, which covers only
    /// what's well past the middle distance, is drawn one frame in
    /// [`FARTHEST_CASCADE_TURN`], on a frame its neighbour isn't
    /// (`AXCENITY_FAR_CASCADE_EVERY_OTHER` puts it back on every other
    /// frame, to compare). A cascade waiting its turn goes in `uniform` as
    /// it was last drawn, for the shaders to read it the way it was drawn.
    fn schedule_cascades(
        &mut self,
        list: &DrawList,
        uniform: &mut FrameUniform,
        aspect: f32,
    ) -> [bool; SHADOW_CASCADES] {
        let mut drawn = [true; SHADOW_CASCADES];
        let Some((_, size)) = shadows_drawn(list.shadows, &self.graphics) else {
            return drawn;
        };
        let cascades = size.cascades as usize;
        let map_fits = self.shadow_map.as_ref().map(|map| map.size) == Some(size);
        let heavy = self.casters_planned > HEAVY_SHADOW_CASTERS;
        // The nearest cascades that ray traced shadows make unneeded cover
        // nothing (`shadow_setup`) and aren't drawn.
        let under_rays = super::lighting::cascades_under_rays(list, &self.graphics, aspect);
        for (index, drawn) in drawn.iter_mut().enumerate().take(cascades) {
            if index < under_rays {
                *drawn = false;
                self.cascades_drawn[index] = None;
                continue;
            }
            let takes_turns = heavy && map_fits && cascades >= 3 && index >= cascades - 2;
            let its_turn = cascade_turn(self.frame as usize, index, cascades);
            // A cascade reaches half the map's width from the camera: it
            // isn't kept once the camera has gone a good way across it.
            let near = |kept: &DrawnCascade| {
                kept.camera.distance(list.camera.position) <= uniform.cascade_texels[index] * 256.0
            };
            match self.cascades_drawn[index] {
                Some(kept) if takes_turns && !its_turn && near(&kept) => {
                    *drawn = false;
                    uniform.cascades[index] = kept.matrix;
                    uniform.cascade_texels[index] = kept.texel;
                    uniform.cascade_depths[index] = kept.depth;
                }
                _ => {
                    self.cascades_drawn[index] = Some(DrawnCascade {
                        matrix: uniform.cascades[index],
                        texel: uniform.cascade_texels[index],
                        depth: uniform.cascade_depths[index],
                        camera: list.camera.position,
                    });
                }
            }
        }
        drawn
    }

    /// Sets `pass` up to draw `batch`, in the scene pass, the depth fill
    /// or the depth and normal prepass (`kind`): its pipeline, its material
    /// or terrain binding, and its mesh, which is returned. `None` skips
    /// it: a caller's mesh with broken data was never uploaded.
    fn bind_batch(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &Batch,
        kind: BatchPass,
    ) -> Option<&GpuMesh> {
        let prepass = kind == BatchPass::Prepass;
        let mesh = self.gpu_mesh(batch.mesh)?;
        if batch.material.terrain != terrain::TerrainKey::default() {
            let group = self.terrain.group(&batch.material.terrain)?;
            pass.set_pipeline(if prepass {
                &self.terrain.prepass
            } else {
                &self.terrain.pipeline
            });
            pass.set_bind_group(1, group, &[]);
        } else {
            let surface = batch.shader.and_then(|key| self.surface_shaders.get(key));
            let traced_water = self.water.traced().filter(|_| self.water.copy.is_some());
            let water = &self.water.pipelines;
            let water = match traced_water {
                Some((pipeline, _)) => pipeline,
                None if self.water.copy.is_some() => &water.refracted,
                None => &water.blended,
            };
            let side = usize::from(batch.double_sided);
            let probe = self
                .plant_probe
                .as_ref()
                .filter(|_| kind == BatchPass::Scene)
                .and_then(|probe| probe.pipeline(batch));
            let pipeline = match (prepass, batch.double_sided, surface) {
                _ if kind == BatchPass::DepthFill => &self.prefill_pipelines.depth[side],
                // Water is never in the prepass.
                _ if batch.water => water,
                _ if probe.is_some() => probe?,
                (false, _, None) if batch.prefilled => &self.prefill_pipelines.lit[side],
                (false, _, None) if !batch.transparent && !batch.fading && self.whole_solids => {
                    &self.prefill_pipelines.whole[side]
                }
                (false, _, Some(surface)) => surface.items.of(batch),
                (false, _, None) => self.item_pipelines.of(batch),
                (true, double_sided, Some(surface)) => surface.prepass(double_sided),
                (true, false, None) => &self.occlusion.prepass,
                (true, true, None) => &self.occlusion.double_sided_prepass,
            };
            let material = self
                .materials
                .get(&batch.material)
                .and_then(|cached| cached.gpu.as_ref())
                .unwrap_or(&self.plain_material);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, &material.bind_group, &[]);
            pass.set_bind_group(2, self.skin_group(mesh), &[]);
            if batch.water
                && kind != BatchPass::DepthFill
                && let Some((_, rays)) = traced_water
            {
                pass.set_bind_group(3, rays, &[]);
            }
        }
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), mesh.index_format);
        Some(mesh)
    }

    /// The group 2 binding `mesh` is drawn with: empty since CO8, as
    /// every pass reads posed corners through the frame's binding 9.
    pub(super) fn skin_group<'a>(&'a self, _mesh: &'a GpuMesh) -> &'a wgpu::BindGroup {
        &self.skinning.none
    }

    /// Remakes the frame bindings with the shadow maps, ambient occlusion
    /// and prepass depth alive, binding the empty stand-in for each that
    /// isn't, so a freed map or target isn't kept alive by the bindings,
    /// and with the frame's posed corners (the shadow passes' bindings too).
    /// A bind group of `layout`, which holds only `bindings` of the frame's
    /// (of its uniform 0, lamps 3, sky 10 to 12 and light probes 16): for
    /// compute passes that would pass eight storage buffers a stage with
    /// all of them.
    pub(super) fn frame_subset(
        &self,
        layout: &wgpu::BindGroupLayout,
        bindings: &[u32],
    ) -> wgpu::BindGroup {
        let (environment, sky_image) = self.environment.views();
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&binding| wgpu::BindGroupEntry {
                binding,
                resource: match binding {
                    0 => self.frame_buffer.as_entire_binding(),
                    3 => self.light_buffer.as_entire_binding(),
                    10 => wgpu::BindingResource::TextureView(environment),
                    11 => wgpu::BindingResource::Sampler(&self.environment.sampler),
                    12 => wgpu::BindingResource::TextureView(sky_image),
                    16 => self.light_probes.buffer().as_entire_binding(),
                    _ => unreachable!("frame binding {binding} isn't offered on its own"),
                },
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame (part)"),
            layout,
            entries: &entries,
        })
    }

    /// Writes the snow's print map into `weather_field` from entry `first`
    /// on: the rows that changed since the version the buffer holds, or all
    /// of it when it holds another map, or this one somewhere else.
    pub(super) fn upload_weather_prints(&mut self, map: &crate::WeatherPrintCells, first: usize) {
        let had = match self.weather_prints_uploaded {
            Some((key, version, at)) if key == map.key && at == first => Some(version),
            _ => None,
        };
        if had == Some(map.version) {
            return;
        }
        let side = map.size as usize;
        let row_bytes = (side * size_of::<f32>()) as u64;
        let start = (first * size_of::<[f32; 4]>()) as u64;
        let changed = |row: usize| had.is_none_or(|had| map.row_versions[row] > had);
        // Each run of changed rows is one write.
        let mut row = 0;
        while row < side {
            if !changed(row) {
                row += 1;
                continue;
            }
            let from = row;
            while row < side && changed(row) {
                row += 1;
            }
            let bytes: &[u8] = bytemuck::cast_slice(&map.cells[from * side..row * side]);
            self.queue
                .write_buffer(&self.weather_field, start + from as u64 * row_bytes, bytes);
            self.weather_print_bytes += bytes.len() as u64;
        }
        self.weather_prints_uploaded = Some((map.key, map.version, first));
    }

    pub(super) fn bind_frame(&mut self) {
        let empty = &self.empty_shadow_view;
        let sun = self.shadow_map.as_ref().map_or(empty, |map| &map.array);
        let spot = self
            .spot_shadow_map
            .as_ref()
            .map_or(empty, |map| &map.array);
        let point = self
            .point_shadow_map
            .as_ref()
            .map_or(empty, |map| &map.array);
        let prepass = self.frame_targets.occlusion.as_ref();
        let occlusion = prepass
            .filter(|_| {
                self.graphics.ambient_occlusion != crate::AmbientOcclusion::Off
                    || ray_ao::traced(&self.graphics)
            })
            .map_or(&self.no_occlusion_view, |targets| &targets.blurred_view);
        let prepass_depth = prepass
            .filter(|_| self.graphics.contact_shadows || self.decals)
            .map_or(&self.no_prepass_depth_view, |targets| &targets.depth_view);
        let (environment, sky_image) = self.environment.views();
        let probes = self.probes.view();
        let ray_shadow_mask = self
            .ray_shadows
            .as_ref()
            .map_or(&self.no_ray_shadows, |shadows| &shadows.mask);
        // Ray traced reflections, or screen-space ones in their place.
        let ray_reflections = match (&self.ray_reflections, &self.screen_reflections) {
            (Some(traced), _) => &traced.buffer,
            (None, Some(screen)) => &screen.buffer,
            (None, None) => &self.no_ray_reflections,
        };
        // Compute layouts (volumetric fog's) leave the foliage table out.
        let bind = |layout, prepass_depth, occlusion, foliage| {
            create_frame_bind_group(
                &self.device,
                layout,
                foliage,
                [
                    &self.frame_buffer,
                    &self.light_buffer,
                    &self.tile_buffer,
                    self.posing.posed(),
                    self.light_probes.buffer(),
                    ray_shadow_mask,
                    ray_reflections,
                    &self.weather_field,
                ],
                [
                    sun,
                    spot,
                    occlusion,
                    environment,
                    sky_image,
                    probes,
                    point,
                    prepass_depth,
                ],
                [
                    &self.shadow_sampler,
                    &self.occlusion_sampler,
                    &self.environment.sampler,
                ],
            )
        };
        let foliage = Some(self.foliage.buffer());
        self.frame_bind_group = bind(&self.frame_layout, prepass_depth, occlusion, foliage);
        self.prepass_frame_bind_group = bind(
            &self.frame_layout,
            &self.no_prepass_depth_view,
            occlusion,
            foliage,
        );
        // Water reads its copy of the scene in ambient occlusion's place.
        self.water_frame_bind_group = self
            .water
            .copy
            .as_ref()
            .map(|copy| bind(&self.frame_layout, prepass_depth, &copy.view, foliage));
        let fog_group = self
            .volumetric_fog
            .as_ref()
            .map(|fog| bind(&fog.frame_layout, prepass_depth, occlusion, None));
        let ray_gi_group = self
            .ray_gi
            .as_ref()
            .map(|gi| self.frame_subset(&gi.frame_layout, &ray_gi::FRAME_BINDINGS));
        if let Some(fog) = &mut self.volumetric_fog {
            fog.frame_group = fog_group;
        }
        if let Some(gi) = &mut self.ray_gi {
            gi.frame_group = ray_gi_group;
        }
        let ray_reflections_group = self
            .ray_reflections
            .as_ref()
            .map(|pass| self.frame_subset(&pass.frame_layout, &ray_gi::FRAME_BINDINGS));
        if let Some(pass) = &mut self.ray_reflections {
            pass.frame_group = ray_reflections_group;
        }
        #[cfg(feature = "debug-views")]
        self.bind_path_tracer();
        self.shadow_bind_group = create_shadow_bind_group(
            &self.device,
            &self.shadow_layout,
            &self.frame_buffer,
            &self.cascade_buffer,
            (self.posing.posed(), self.foliage.buffer()),
        );
    }

    /// How many shadow maps are alive: 0 or 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn shadow_maps_alive(&self) -> usize {
        Arc::strong_count(&self.shadow_maps_alive) - 1
    }

    /// How many spot shadow maps are alive: 0 or 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn spot_shadow_maps_alive(&self) -> usize {
        Arc::strong_count(&self.spot_shadow_maps_alive) - 1
    }

    /// How many point shadow maps are alive: 0 or 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn point_shadow_maps_alive(&self) -> usize {
        Arc::strong_count(&self.point_shadow_maps_alive) - 1
    }

    /// How many light probes real-time GI has traced, or `None` while it
    /// has nothing to trace (it's off, or no volumes are drawn).
    #[cfg(test)]
    pub(super) fn probes_traced(&self) -> Option<u32> {
        self.realtime_gi.as_ref().map(|gi| gi.traced)
    }

    /// How many probes ray traced GI has traced, while it's on.
    #[cfg(test)]
    pub(super) fn probes_ray_traced(&self) -> Option<u64> {
        self.ray_gi.as_ref().map(|gi| gi.probes_traced)
    }

    /// How far lighting.wgsl's `shading_noise` moves the shadow filters'
    /// and contact shadows' per-pixel turn in the next frame
    /// (`Frame::noise.x`). With TAA it's the frame's index (of 64), so the
    /// turn changes every frame and TAA averages it into a smooth edge.
    /// Without TAA it's 0: nothing would average a moving turn, so it
    /// would flicker, and a still scene draws the same pixels every frame.
    pub(super) fn shading_noise_step(&self) -> f32 {
        if self.taa_on() {
            (self.history.frame_index() % 64) as f32
        } else {
            0.0
        }
    }

    /// Whether TAA blends this frame with the last ones.
    fn taa_on(&self) -> bool {
        self.frame_targets
            .temporal
            .as_ref()
            .is_some_and(|temporal| temporal.has_taa())
    }

    /// How many sets of scene targets are alive: 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn targets_alive(&self) -> usize {
        Arc::strong_count(&self.targets_alive.sets) - 1
    }

    /// How many sets of prepass and ambient occlusion targets are alive: 0
    /// or 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn occlusion_targets_alive(&self) -> usize {
        Arc::strong_count(&self.targets_alive.occlusion) - 1
    }

    /// How many of depth of field's blurred copies of the scene are alive:
    /// 0 or 1, unless one leaked.
    #[cfg(test)]
    pub(super) fn depth_of_field_targets_alive(&self) -> usize {
        Arc::strong_count(&self.targets_alive.depth_of_field) - 1
    }

    /// How many sets of volumetric fog grids are alive: 0 or 1, unless one
    /// leaked.
    #[cfg(test)]
    pub(super) fn volumetric_fogs_alive(&self) -> usize {
        Arc::strong_count(&self.targets_alive.volumetric_fog) - 1
    }

    /// How many GPU particle emitters have buffers.
    #[cfg(test)]
    pub(super) fn gpu_particle_emitters(&self) -> usize {
        self.gpu_particles.as_ref().map_or(0, |p| p.emitters())
    }

    /// How many of water's copies of the scene are alive: 0 or 1, unless
    /// one leaked.
    #[cfg(test)]
    pub(super) fn water_copies_alive(&self) -> usize {
        Arc::strong_count(&self.targets_alive.water) - 1
    }

    /// How many sets of motion targets, TAA histories and motion blur
    /// targets are alive: 0 or 1 each, unless one leaked.
    #[cfg(test)]
    pub(super) fn temporal_targets_alive(&self) -> (usize, usize, usize) {
        let alive = &self.targets_alive;
        (
            Arc::strong_count(&alive.motion) - 1,
            Arc::strong_count(&alive.taa) - 1,
            Arc::strong_count(&alive.motion_blur) - 1,
        )
    }

    /// How many multisampled and FXAA targets are alive: 0 or 1 each.
    #[cfg(test)]
    pub(super) fn anti_aliasing_targets_alive(&self) -> (usize, usize) {
        let alive = &self.targets_alive;
        (
            Arc::strong_count(&alive.msaa) - 1,
            Arc::strong_count(&alive.fxaa) - 1,
        )
    }

    /// Times each frame from its start ([`Self::frame_started`]) to the
    /// GPU finishing it, while `on` (renderer/latency.rs).
    pub(super) fn watch_latency(&mut self, on: bool) {
        match (on, &self.latency) {
            (true, None) => self.latency = Some(latency::LatencyWatch::new(&self.device)),
            (false, Some(_)) => self.latency = None,
            _ => {}
        }
    }

    /// The next frame drawn started at `at`, for the latency watch.
    pub(super) fn frame_started(&mut self, at: std::time::Instant) {
        if let Some(latency) = &mut self.latency {
            latency.frame_started(at);
        }
    }

    /// The watched frames' times since last asked.
    pub(super) fn take_latencies(&self) -> Vec<Duration> {
        self.latency.as_ref().map(|l| l.take()).unwrap_or_default()
    }

    /// Notes this frame's submission and, one frame ahead
    /// (`TextureRenderer::set_one_frame_ahead`), waits for the frame
    /// before it to finish on the GPU. The GPU then always has this frame
    /// to go on with and never a third behind it: a host that starts each
    /// frame as the last one is handed over is one frame of GPU work from
    /// the picture, however many the swapchain would queue (four, measured
    /// on radv at 3840 × 2160).
    fn wait_for_frame_before(&mut self, submitted: wgpu::SubmissionIndex) {
        /// A GPU that takes longer than this for a frame has stopped.
        const LONGEST: Duration = Duration::from_secs(2);
        self.gpu_wait = Duration::ZERO;
        let before = self.last_frame.replace(submitted);
        if !self.one_frame_ahead {
            return;
        }
        let Some(before) = before else {
            return;
        };
        let waiting = std::time::Instant::now();
        // A timeout or a lost device shows up on the next submit.
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(before),
            timeout: Some(LONGEST),
        });
        self.gpu_wait = waiting.elapsed();
    }

    pub(super) fn stats(&self) -> RenderStats {
        let counts = self.counts;
        // A lamp's map with its kept still casters.
        let kept =
            |kept: &Option<lamp_shadows::KeptLayers>| kept.as_ref().map_or(0, |kept| kept.bytes);
        let shadow_bytes = self.shadow_map.as_ref().map_or(0, |map| map.bytes)
            + (self.spot_shadow_map.as_ref()).map_or(0, |map| map.bytes + kept(&map.kept))
            + (self.point_shadow_map.as_ref()).map_or(0, |map| map.bytes + kept(&map.kept));
        let sheet_bytes = self.turned_text.sheet.as_ref().map_or(0, |(sheet, _, _)| {
            target_bytes((sheet.width(), sheet.height(), 1), sheet.format(), 1)
        });
        let buffers = [
            &self.instance_buffer,
            &self.skinning.joints,
            &self.ui_buffer,
            &self.light_buffer,
            &self.tile_buffer,
            &self.frame_buffer,
            &self.cascade_buffer,
            self.light_probes.buffer(),
        ];
        let (surface_shaders, broken_shaders) = self.surface_shaders.counts();
        RenderStats {
            gpu_wait: self.gpu_wait,
            cpu_times: self.cpu_times,
            draw_calls: counts.draw_calls,
            command_buffers: counts.command_buffers,
            triangles: counts.triangles,
            vertices: counts.vertices,
            gpu_time: self
                .gpu_timer
                .as_ref()
                .filter(|_| self.time_gpu)
                .and_then(gpu_timer::GpuTimer::last),
            gpu_timing: self.gpu_timer.is_some() && self.time_gpu,
            gpu_readings: self
                .gpu_timer
                .as_ref()
                .map_or(0, gpu_timer::GpuTimer::readings),
            cascade_times: self
                .gpu_timer
                .as_ref()
                .filter(|_| self.time_gpu)
                .map_or([None; SHADOW_CASCADES], gpu_timer::GpuTimer::last_cascades),
            cascade_casters: counts.cascade_casters,
            cascade_triangles: counts.cascade_triangles,
            cascade_draws: counts.cascade_draws,
            lamp_shadow_draws: counts.lamp_shadow_draws,
            lamp_views_kept: counts.lamp_views_kept,
            pass_times: self
                .gpu_timer
                .as_ref()
                .filter(|_| self.time_gpu)
                .map_or([None; FramePass::COUNT], gpu_timer::GpuTimer::last_passes),
            part_times: self
                .gpu_timer
                .as_ref()
                .filter(|_| self.time_gpu)
                .map_or_else(Default::default, gpu_timer::GpuTimer::last_parts),
            items_submitted: counts.items_submitted,
            items_drawn: counts.items_drawn,
            batches: counts.batches,
            prepass_draws: counts.prepass_draws,
            motion_draws: counts.motion_draws,
            depth_fill_draws: counts.depth_fill_draws,
            scattered_batches: counts.scattered_batches,
            scattered_items: counts.scattered_items,
            depth_from_prepass: counts.depth_from_prepass,
            items_culled: counts.items_culled,
            shadow_casters_drawn: counts.shadow_casters_drawn,
            shadow_casters_culled: counts.shadow_casters_culled,
            sprites_culled: counts.sprites_culled,
            decals_drawn: counts.decals_drawn,
            decals_left_out: counts.decals_left_out,
            lod_items: counts.lod_items,
            surface_shaders,
            broken_shaders,
            gpu_particles: self.gpu_particles.as_ref().map_or(0, |p| p.count()),
            ray_tracing: self
                .ray_scene
                .as_ref()
                .map_or_else(Default::default, |scene| {
                    #[allow(unused_mut)]
                    let mut stats = scene.stats;
                    #[cfg(feature = "debug-views")]
                    {
                        stats.path_traced_paths = self.path_traced_paths();
                    }
                    stats
                }),
            posing: self.posing.stats(),
            texture_bytes: self.texture_bytes.total(),
            mesh_bytes: self.mesh_bytes.total(),
            texture_bytes_wanted: self.texture_bytes_wanted(),
            freed_for_budget: self.freed_for_budget,
            target_bytes: self.frame_targets.bytes
                + self.cloud_trace.bytes()
                + self.foliage.bytes()
                + shadow_bytes
                + sheet_bytes
                + self.environment.bytes()
                + self.probes.bytes()
                + self.realtime_gi.as_ref().map_or(0, |gi| gi.bytes())
                + self.ray_gi.as_ref().map_or(0, |gi| gi.bytes())
                + self.ray_reflections.as_ref().map_or(0, |pass| pass.bytes())
                + self
                    .screen_reflections
                    .as_ref()
                    .map_or(0, |pass| pass.bytes())
                + self.soft_scene.as_ref().map_or(0, |soft| soft.bytes())
                + self.volumetric_fog.as_ref().map_or(0, |fog| fog.bytes())
                + self.water.copy.as_ref().map_or(0, |copy| copy.bytes),
            buffer_bytes: buffers.iter().map(|buffer| buffer.size()).sum::<u64>()
                + self.posing.stats().bytes
                + self.gpu_particles.as_ref().map_or(0, |p| p.bytes())
                + self
                    .ray_shadows
                    .as_ref()
                    .map_or(0, |shadows| shadows.bytes()),
        }
    }

    /// The box a mesh's corners span, for culling. A caller's mesh that
    /// was never uploaded isn't drawn, so the unit cube does for it.
    #[cfg(test)]
    pub(super) fn mesh_bounds(&self, key: MeshKey) -> Bounds {
        self.gpu_mesh(key).map_or(Bounds::UNIT, |mesh| mesh.bounds)
    }

    /// The box an item's mesh spans, in its own space, once its `skin` (if
    /// any) moves its corners.
    #[cfg(test)]
    pub(super) fn item_bounds(&self, key: MeshKey, skin: Option<&Skin>) -> Bounds {
        item_bounds(&self.meshes, &self.custom_meshes, key, skin)
    }

    /// The GPU copy of a mesh, or `None` for a caller's mesh that was never uploaded.
    /// The corners `skin` drawn with `mesh` was posed into last frame
    /// (CO8), read back: each one's position, normal and tangent.
    #[cfg(test)]
    pub(super) fn read_posed(
        &self,
        skin: &crate::Skin,
        mesh: MeshKey,
    ) -> Option<Vec<[glam::Vec3; 3]>> {
        let first = self.posing.offset(skin, mesh)?;
        let count = self.gpu_mesh(mesh)?.vertex_count;
        Some(self.posing.read((&self.device, &self.queue), first, count))
    }

    pub(super) fn gpu_mesh(&self, key: MeshKey) -> Option<&GpuMesh> {
        gpu_mesh_in(&self.meshes, &self.custom_meshes, key)
    }

    /// The binding a shadow view draws a cut-out caster through: its
    /// material (`material`, its index in `uses`), or the plain one for a
    /// caster without holes ([`lighting::CutOutCaster::NO_HOLES`]) or a
    /// material not bound yet.
    pub(super) fn cut_out_material(&self, uses: &FrameUses, material: u32) -> &GpuMaterial {
        uses.materials
            .get(material as usize)
            .and_then(|key| self.materials.get(key))
            .and_then(|cached| cached.gpu.as_ref())
            .unwrap_or(&self.plain_material)
    }

    /// Shapes and uploads the UI text in `ui`, with its rich text runs,
    /// shadows, outlines and faux bold, into one text renderer per layer.
    /// Returns the shaped text, which must live until it's drawn, its
    /// underlines and line-throughs by the index of the text in `ui`, and
    /// which `layers` have text.
    pub(super) fn prepare_ui_text(
        &mut self,
        ui: &[UiDrawItem],
        layers: &[Range<usize>],
        (width, height): (u32, u32),
    ) -> Result<(Vec<Buffer>, Vec<TextLine>, Vec<bool>)> {
        self.text_viewport
            .update(&self.queue, Resolution { width, height });

        let text_items: Vec<(usize, &UiText)> = ui
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                // Turned text is drawn from the sheet instead.
                UiDrawItem::Text(text) if !is_turned(text.rotation_degrees) => Some((index, text)),
                UiDrawItem::Text(_) | UiDrawItem::Rectangle(_) | UiDrawItem::Image(_) => None,
            })
            .collect();
        if self.fitted_sizes.len() > 4096 {
            self.fitted_sizes.clear();
        }
        let mut buffers = Vec::new();
        // Each copy to draw: the text, where its lines start, the buffer
        // it's shaped in, and the pass.
        let mut placed = Vec::new();
        let mut lines = Vec::new();
        for &(index, item) in &text_items {
            let shaped = self.shape_ui_text(item, &mut buffers);
            lines.extend(shaped.lines.into_iter().map(|line| {
                (
                    index,
                    UiRectangle {
                        clip: item.clip,
                        ..line
                    },
                )
            }));
            placed.extend(
                shaped
                    .passes
                    .into_iter()
                    .map(|pass| (index, item, shaped.top, shaped.first + pass.layer, pass)),
            );
        }

        let mut with_text = vec![false; layers.len()];
        for (layer, range) in layers.iter().enumerate() {
            let in_layer: Vec<_> = placed
                .iter()
                .filter(|(index, ..)| range.contains(index))
                .collect();
            if in_layer.is_empty() {
                continue;
            }
            with_text[layer] = true;
            while self.text_renderers.len() <= layer {
                self.text_renderers.push(GlyphTextRenderer::new(
                    &mut self.text_atlas,
                    &self.device,
                    wgpu::MultisampleState::default(),
                    None,
                ));
            }
            let areas = in_layer.into_iter().map(|&(_, text, top, buffer, pass)| {
                let [dx, dy] = pass.offset;
                let mut bounds = text_bounds(text, pass.offset);
                // Cut to the clip too, which doesn't move with a shadow.
                if let Some(clip) = text.clip {
                    let [left, upper, right, lower] = super::ui_pass::clip_bounds(clip);
                    bounds.left = bounds.left.max(left.floor() as i32);
                    bounds.top = bounds.top.max(upper.floor() as i32);
                    bounds.right = bounds.right.min(right.ceil() as i32);
                    bounds.bottom = bounds.bottom.min(lower.ceil() as i32);
                    bounds.right = bounds.right.max(bounds.left);
                    bounds.bottom = bounds.bottom.max(bounds.top);
                }
                TextArea {
                    buffer: &buffers[buffer],
                    left: text.position[0] + dx,
                    top: top + dy,
                    scale: 1.0,
                    // Where it may show, in screen pixels (not relative to
                    // the text's position): its box, moved with the copy so
                    // a shadow is cut where its letters are.
                    bounds,
                    default_color: glyph_color(pass.color),
                    custom_glyphs: &[],
                }
            });
            self.text_renderers[layer]
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.text_atlas,
                    &self.text_viewport,
                    areas,
                    &mut self.swash_cache,
                )
                .context("could not prepare UI text")?;
        }
        Ok((buffers, lines, with_text))
    }

    /// Shapes one UI text into `buffers`, one buffer per paint (see
    /// `text::passes`), and works out its copies and decorations.
    pub(super) fn shape_ui_text(&mut self, item: &UiText, buffers: &mut Vec<Buffer>) -> ShapedText {
        let styled = text::Styled::new(self.font_system.db(), item);
        let font_size = if item.scaled {
            let runs: Vec<_> = styled
                .runs
                .iter()
                .map(|run| (&run.text, &run.font_family, run.font_size))
                .collect();
            let key = format!(
                "{:?}{:?}{:?}{:?}",
                styled.faces,
                runs,
                item.size,
                (
                    &item.font_family,
                    item.font_size,
                    item.letter_spacing,
                    item.line_height,
                    item.wrap,
                )
            );
            match self.fitted_sizes.get(&key) {
                Some(size) => *size,
                None => {
                    let size = text::fitted_size(&mut self.font_system, &styled);
                    self.fitted_sizes.insert(key, size);
                    size
                }
            }
        } else {
            item.font_size.max(1.0)
        };
        let (layers, passes) = text::passes(&styled, font_size);
        let first = buffers.len();
        for paint in &layers {
            buffers.push(text::shape(
                &mut self.font_system,
                &styled,
                font_size,
                paint,
            ));
        }
        let top = item.position[1] + text::vertical_offset(item, &buffers[first]);
        let lines = text::decorations(&styled, &buffers[first], font_size, [item.position[0], top]);
        ShapedText {
            first,
            top,
            passes,
            lines,
        }
    }

    /// Uploads the callers' meshes in `draws` (each item's level of
    /// detail) and textures in `list` that aren't on the GPU yet, and notes
    /// that the rest were drawn this frame. Textures are fitted to the
    /// texture budget before the materials using them are bound.
    /// `uses` says what `draws` use.
    pub(super) fn upload_new(&mut self, list: &DrawList, draws: &[Draw], uses: &FrameUses) {
        let frame = self.frame;
        let arrived = self.finish_background_uploads();
        // Skirts made on a worker join their meshes: the indices again, the
        // skirt's after them (the mesh's own are the same, so nothing built
        // from the old buffer is out of date).
        for made in self.skirts.made() {
            if let Some(Cached { gpu: Some(gpu), .. }) = self.custom_meshes.get_mut(&made.key)
                && gpu.version == made.version
                && made.indices.len() > gpu.index_count as usize
            {
                gpu.indices =
                    index_buffer(&self.device, "mesh", bytemuck::cast_slice(&made.indices));
                gpu.cap_index_count = made.indices.len() as u32;
                gpu._bytes = self
                    .mesh_bytes
                    .count(gpu.vertices.size() + gpu.indices.size());
            }
        }
        // Terrain chunks and painted terrain water meshes are small and rebuilt
        // live as the camera moves or the user paints: each goes up right away,
        // like terrain weights. Left to the background upload thread, a chunk
        // switching LOD or a water chunk being painted would disappear for a
        // few frames and flicker.
        let terrain_meshes: rustc_hash::FxHashSet<u64> = match self.uploader {
            Some(_) => list
                .items
                .iter()
                .filter(|item| item.material.terrain.is_some() || item.material.water.is_some())
                .filter_map(|item| match &item.shape {
                    Shape::Mesh(source) => Some(source.key()),
                    _ => None,
                })
                .collect(),
            None => Default::default(),
        };
        // Thousands of draws share a few meshes and textures: each is
        // looked up once rather than once per draw.
        for &source in &uses.meshes {
            match self.custom_meshes.entry(source.key()) {
                Entry::Occupied(mut cached) => {
                    let cached = cached.get_mut();
                    cached.last_drawn = frame;
                    // A posed (animated) mesh has new corners: rewrite them.
                    if let Some(gpu) = &mut cached.gpu
                        && gpu.version != source.version()
                    {
                        update_custom_mesh(
                            &self.device,
                            &self.queue,
                            (&self.mesh_bytes, &self.skinning),
                            gpu,
                            source.as_ref(),
                        );
                    }
                }
                Entry::Vacant(slot) => {
                    if let Some(uploader) = &mut self.uploader
                        && !terrain_meshes.contains(&source.key())
                    {
                        // Not drawn until its thread is done with it.
                        uploader.mesh(source);
                        continue;
                    }
                    // A renderer whose frames are shown makes the skirt
                    // away from the frame; an offline one (tests, stills)
                    // draws the frame asked for with it.
                    let later = self.one_frame_ahead;
                    slot.insert(Cached {
                        gpu: upload_custom_mesh_with(
                            &self.device,
                            &self.mesh_bytes,
                            &self.skinning,
                            source.as_ref(),
                            !later,
                        ),
                        last_drawn: frame,
                    });
                    if later {
                        self.skirts.request(Arc::clone(source));
                    }
                }
            }
        }
        for draw in draws {
            if let Some(shader) = &draw.item.material.surface_shader {
                self.prepare_surface_shader(shader);
            }
        }
        for item in &list.items {
            if let Some(surface) = &item.material.terrain {
                for weights in &surface.weights {
                    self.note_texture(weights, frame, true);
                }
                let weights = surface.weights.each_ref().map(|source| {
                    self.full_textures
                        .get(&source.key())
                        .and_then(|cached| cached.gpu.as_ref())
                });
                self.terrain.prepare(
                    &self.device,
                    &self.queue,
                    surface,
                    weights,
                    &self.white_texture,
                    frame,
                );
                continue;
            }
            for source in material_textures(item).into_iter().flatten() {
                self.note_texture(source, frame, false);
            }
        }
        // Each material's textures are noted once. The list's own items
        // are done above: this is for what's in its groups.
        for (key, &first) in uses.materials.iter().zip(&uses.first_draw) {
            if *key == MaterialKey::default() {
                continue;
            }
            for source in material_textures(draws[first as usize].item)
                .into_iter()
                .flatten()
            {
                self.note_texture(source, frame, false);
            }
        }
        for item in &list.ui {
            if let UiDrawItem::Image(image) = item
                && let Some(source) = &image.texture
            {
                self.note_texture(source, frame, true);
            }
        }
        let emitter_textures = list.particle_emitters.iter().map(|e| &e.look.texture);
        let sprite_textures = list.sprites.iter().map(|s| &s.texture);
        let decal_textures = list.decals.iter().map(|d| &d.texture);
        for source in sprite_textures
            .chain(emitter_textures)
            .chain(decal_textures)
            .flatten()
        {
            self.note_texture(source, frame, false);
        }
        // What isn't being drawn makes room first (while it's kept at all:
        // renderer/undrawn.rs), then what is drawn loses levels.
        let mut replaced = self.free_undrawn_textures();
        replaced.extend(self.fit_texture_budget(list));
        replaced.extend(arrived);
        if !replaced.is_empty() {
            self.materials.retain(|key, _| {
                !key.textures
                    .iter()
                    .flatten()
                    .any(|texture| replaced.contains(texture))
            });
            self.sprites.forget(|key| !replaced.contains(&key));
            self.decal_pass.forget(|key| !replaced.contains(&key));
        }
        for (key, &first) in uses.materials.iter().zip(&uses.first_draw) {
            let key = *key;
            if key == MaterialKey::default() {
                continue;
            }
            let item = draws[first as usize].item;
            if item.material.terrain.is_some() {
                continue;
            }
            let sources = material_textures(item);
            match self.materials.entry(key) {
                Entry::Occupied(mut cached) => cached.get_mut().last_drawn = frame,
                Entry::Vacant(slot) => {
                    // A texture with broken data is drawn as white instead.
                    let textures = sources.map(|source| {
                        source
                            .and_then(|source| self.textures.get(&source.key())?.gpu.as_ref())
                            .unwrap_or(&self.white_texture)
                    });
                    slot.insert(Cached {
                        gpu: Some(create_material(
                            &self.device,
                            &self.material_layout,
                            &self.sampler,
                            textures,
                            maps_uniform_of(&key),
                        )),
                        last_drawn: frame,
                    });
                }
            }
        }
    }

    /// Appends the quads of the UI images in `ui[range]`, and of its turned
    /// text (`turned`, by index in `ui`), and remembers which texture each
    /// uses: [`SHEET_KEY`] for turned text.
    pub(super) fn ui_image_draws(
        &mut self,
        ui: &[UiDrawItem],
        range: Range<usize>,
        turned: &HashMap<usize, Vec<UiVertex>>,
        viewport: (u32, u32),
        vertices: &mut Vec<UiVertex>,
    ) -> Vec<(u64, u32, u32)> {
        let mut draws = Vec::new();
        for index in range {
            if let Some(quad) = turned.get(&index) {
                let start = vertices.len() as u32;
                vertices.extend_from_slice(quad);
                draws.push((SHEET_KEY, start, quad.len() as u32));
                continue;
            }
            let UiDrawItem::Image(image) = &ui[index] else {
                continue;
            };
            let Some(source) = &image.texture else {
                continue;
            };
            let key = source.key();
            if self
                .full_textures
                .get(&key)
                .and_then(|cached| cached.gpu.as_ref())
                .is_none()
            {
                continue;
            }
            if !self.ui_image_groups.contains_key(&key) {
                let group = {
                    let gpu = self
                        .full_textures
                        .get(&key)
                        .and_then(|cached| cached.gpu.as_ref())
                        .expect("checked just above");
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("UI image"),
                        layout: &self.ui_image_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&gpu.srgb),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.ui_sampler),
                            },
                        ],
                    })
                };
                self.ui_image_groups.insert(key, group);
            }
            let quad = image_quad(image, viewport);
            if quad.is_empty() {
                continue;
            }
            let start = vertices.len() as u32;
            let count = quad.len() as u32;
            vertices.extend(quad);
            draws.push((key, start, count));
        }
        draws
    }

    /// Uploads `source` the first time, and marks it drawn this frame. A
    /// `full` texture (a UI image or terrain weights) keeps its full size;
    /// others are sized by the texture quality.
    pub(super) fn note_texture(&mut self, source: &Arc<dyn TextureSource>, frame: u64, full: bool) {
        let (cache, skipped_levels) = if full {
            (&mut self.full_textures, 0)
        } else {
            (
                &mut self.textures,
                self.graphics.texture_quality.skipped_levels(),
            )
        };
        match cache.entry(source.key()) {
            Entry::Occupied(mut cached) => cached.get_mut().last_drawn = frame,
            Entry::Vacant(slot) => {
                let sizing = (skipped_levels, self.capabilities.max_texture_size);
                // Terrain weights and UI images are small: done right away.
                if !full && let Some(uploader) = &mut self.uploader {
                    uploader.texture(source, sizing);
                    return;
                }
                slot.insert(Cached {
                    gpu: upload_texture(
                        &self.device,
                        &self.queue,
                        &self.texture_bytes,
                        source.as_ref(),
                        sizing,
                        0,
                        // UI images and terrain weights stay exact.
                        !full,
                    ),
                    last_drawn: frame,
                });
            }
        }
    }

    /// See [`Renderer::set_background_uploads`](super::Renderer::set_background_uploads).
    pub(super) fn set_background_uploads(&mut self, on: bool) {
        if on && self.uploader.is_none() {
            self.uploader = Some(super::uploads::Uploader::start(
                self.device.clone(),
                self.queue.clone(),
                (self.mesh_bytes.clone(), self.texture_bytes.clone()),
                self.skinning.clone(),
            ));
        } else if !on {
            self.uploader = None;
        }
    }

    /// Asks the upload thread for every mesh (with its levels of detail)
    /// and color texture of `list`'s items, drawn this frame or not, so
    /// turning the camera later finds them on the GPU. Only while
    /// background uploads are on.
    pub(super) fn preload(&mut self, list: &DrawList) {
        if self.uploader.is_none() {
            return;
        }
        let frame = self.frame;
        let chunked = list.chunks.iter().flat_map(|chunk| chunk.items.iter());
        for item in list.items.iter().chain(chunked) {
            if item.material.terrain.is_some() {
                continue;
            }
            if let Shape::Mesh(source) = &item.shape {
                let levels = (1..=source.lod_count()).filter_map(|level| source.lod(level));
                for mesh in std::iter::once(source).chain(levels) {
                    if !self.custom_meshes.contains_key(&mesh.key())
                        && let Some(uploader) = &mut self.uploader
                    {
                        uploader.mesh(mesh);
                    }
                }
            }
            for source in material_textures(item).into_iter().flatten() {
                self.note_texture(source, frame, false);
            }
        }
    }

    /// How many background uploads are still on their way.
    pub(super) fn pending_uploads(&self) -> usize {
        self.uploader
            .as_ref()
            .map_or(0, |uploader| uploader.pending())
    }

    /// Takes in what the background uploads finished, and returns the keys
    /// of the textures among them: materials that drew white for them are
    /// made again.
    fn finish_background_uploads(&mut self) -> Vec<u64> {
        let Some(uploader) = &mut self.uploader else {
            return Vec::new();
        };
        let (meshes, textures) = uploader.take_finished();
        let frame = self.frame;
        for (key, gpu) in meshes {
            self.custom_meshes.insert(
                key,
                Cached {
                    gpu,
                    last_drawn: frame,
                },
            );
        }
        let arrived = textures.iter().map(|(key, _)| *key).collect();
        for (key, gpu) in textures {
            self.textures.insert(
                key,
                Cached {
                    gpu,
                    last_drawn: frame,
                },
            );
        }
        arrived
    }

    /// Replaces the instance buffer with a bigger one if `count` won't fit.
    pub(super) fn ensure_instance_capacity(&mut self, count: usize) {
        if count <= self.instance_capacity {
            return;
        }
        self.instance_capacity = count.next_power_of_two();
        self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
    }

    pub(super) fn ensure_ui_capacity(&mut self, count: usize) {
        if count <= self.ui_capacity {
            return;
        }
        self.ui_capacity = count.next_power_of_two();
        self.ui_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI vertices"),
            size: (self.ui_capacity * size_of::<UiVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
}

/// Begins a scene pass into `targets`: clearing them, or (`load`) carrying
/// on with what an earlier scene pass drew. With MSAA its samples are
/// averaged into the HDR texture as it ends. `keep` keeps the samples and
/// depth for a later pass (water's copy reads the depth); otherwise they're
/// thrown away.
/// More shadow casters a frame than this and the far cascades take turns
/// (see [`ScenePipeline::schedule_cascades`]).
const HEAVY_SHADOW_CASTERS: usize = 12000;

/// With four cascades or more, the farthest is drawn one frame in this
/// many in a heavy scene (Overgrown Street's farthest cascade was about
/// 8 ms of GPU on the frames that drew it, 2026-11-27).
const FARTHEST_CASCADE_TURN: usize = 4;

/// Whether cascade `index` of `cascades`, one of the two farthest, is drawn
/// on `frame` in a heavy scene: the two take alternate frames, and with
/// four cascades or more the farthest takes only one frame in
/// [`FARTHEST_CASCADE_TURN`] of its own, so no frame draws both.
pub(super) fn cascade_turn(frame: usize, index: usize, cascades: usize) -> bool {
    static EVERY_OTHER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let every_other =
        *EVERY_OTHER.get_or_init(|| std::env::var_os("AXCENITY_FAR_CASCADE_EVERY_OTHER").is_some());
    let alternate = (frame + index).is_multiple_of(2);
    if cascades < 4 || index + 1 < cascades || every_other {
        return alternate;
    }
    // The frames its neighbour (index - 1) leaves free are those where
    // `frame + index` is even; one of every two of them.
    alternate && (frame + index).is_multiple_of(FARTHEST_CASCADE_TURN)
}

/// Which pass a batch is being drawn in (see `ScenePipeline::bind_batch`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchPass {
    Scene,
    /// The cut-out surfaces' depth, drawn first in the scene pass.
    DepthFill,
    Prepass,
}

/// The GPU copy of a mesh among `meshes` and `custom_meshes`, or `None` for
/// a caller's mesh that was never uploaded.
pub(super) fn gpu_mesh_in<'a>(
    meshes: &'a Meshes,
    custom_meshes: &'a FxHashMap<u64, Cached<GpuMesh>>,
    key: MeshKey,
) -> Option<&'a GpuMesh> {
    match key {
        MeshKey::Custom(key) => custom_meshes
            .get(&key)
            .and_then(|cached| cached.gpu.as_ref()),
        builtin => meshes.get(builtin),
    }
}

/// The box a mesh spans, in its own space, once its `skin` (if any) moves
/// its corners. A caller's mesh that was never uploaded isn't drawn, so the
/// unit cube does for it.
fn item_bounds(
    meshes: &Meshes,
    custom_meshes: &FxHashMap<u64, Cached<GpuMesh>>,
    key: MeshKey,
    skin: Option<&Skin>,
) -> Bounds {
    let mesh = gpu_mesh_in(meshes, custom_meshes, key);
    match (mesh, skin) {
        (
            Some(GpuMesh {
                skin: Some(gpu), ..
            }),
            Some(skin),
        ) => gpu.bounds(skin),
        _ => mesh.map_or(Bounds::UNIT, |mesh| mesh.bounds),
    }
}

/// Begins a pass drawing into the scene's targets. `load` says whether
/// the color, then the depth, already there is kept (else it's cleared),
/// and `keep` whether a later pass carries on from what this one leaves.
fn begin_scene_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    targets: &'a FrameTargets,
    (load_color, load_depth): (bool, bool),
    keep: bool,
) -> wgpu::RenderPass<'a> {
    let color_load = if load_color {
        wgpu::LoadOp::Load
    } else {
        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
    };
    let depth_load = if load_depth {
        wgpu::LoadOp::Load
    } else {
        wgpu::LoadOp::Clear(1.0)
    };
    let kept = if keep {
        wgpu::StoreOp::Store
    } else {
        wgpu::StoreOp::Discard
    };
    let color_attachment = match &targets.msaa {
        Some(msaa) => wgpu::RenderPassColorAttachment {
            depth_slice: None,
            view: &msaa.view,
            resolve_target: Some(&targets.hdr_view),
            ops: wgpu::Operations {
                load: color_load,
                // Only the averaged result is needed, unless a later pass
                // carries on drawing.
                store: kept,
            },
        },
        None => wgpu::RenderPassColorAttachment {
            depth_slice: None,
            view: &targets.hdr_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
        },
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("scene"),
        color_attachments: &[Some(color_attachment)],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &targets.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: depth_load,
                store: kept,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

/// The scene pass's sky pipeline, its pipelines for items and for their
/// outlines, for drawing with `samples` per pixel, with `layouts` the
/// frame's, the material's and the skin's bindings.
fn scene_pipelines(
    device: &wgpu::Device,
    layouts: [&wgpu::BindGroupLayout; 3],
    clouds_layout: &wgpu::BindGroupLayout,
    samples: u32,
) -> (
    wgpu::RenderPipeline,
    ItemPipelines,
    wgpu::RenderPipeline,
    PrefillPipelines,
) {
    let frame_layout = layouts[0];
    let shader = shader_module(device, "scene", gpu::SCENE_SHADER);
    let pipeline = |transparent, double_sided| {
        create_pipeline(
            device,
            &shader,
            ("vs_main", "fs_main"),
            &layouts,
            (HDR_FORMAT, samples),
            (transparent, double_sided),
        )
    };
    let prefill = |fragment, double_sided, depth| {
        create_pipeline_for(
            device,
            &shader,
            ("vs_main", fragment),
            &layouts,
            (HDR_FORMAT, samples),
            (false, double_sided),
            depth,
        )
    };
    // With one sample per pixel there's nothing for coverage to share out.
    let depth_fill = if samples > 1 {
        "fs_depth_coverage"
    } else {
        "fs_depth_only"
    };
    // Built at once on threads of their own: each compiles its shaders for
    // the GPU (on DirectX 12 through DXC, which takes a while for this
    // big shader), and wgpu lets pipelines be made from any thread.
    let [
        sky,
        opaque,
        transparent,
        double_sided,
        double_sided_transparent,
        outline,
        depth,
        depth_double,
        lit,
        lit_double,
        whole,
        whole_double,
    ] = gpu::in_parallel([
        &|| create_sky_pipeline(device, frame_layout, clouds_layout, samples),
        &|| pipeline(false, false),
        &|| pipeline(true, false),
        &|| pipeline(false, true),
        &|| pipeline(true, true),
        &|| {
            create_pipeline(
                device,
                &shader,
                ("vs_outline", "fs_outline"),
                &layouts,
                (HDR_FORMAT, samples),
                (false, true),
            )
        },
        &|| prefill(depth_fill, false, DepthUse::Fill),
        &|| prefill(depth_fill, true, DepthUse::Fill),
        &|| prefill("fs_prefilled", false, DepthUse::Filled),
        &|| prefill("fs_prefilled", true, DepthUse::Filled),
        &|| prefill("fs_prefilled", false, DepthUse::Plain),
        &|| prefill("fs_prefilled", true, DepthUse::Plain),
    ]);
    (
        sky,
        ItemPipelines {
            opaque,
            transparent,
            double_sided,
            double_sided_transparent,
        },
        outline,
        PrefillPipelines {
            depth: [depth, depth_double],
            lit: [lit, lit_double],
            whole: [whole, whole_double],
        },
    )
}

/// The frame's bindings (group 0 in lighting.wgsl), as the scene's shaders
/// see them.
fn frame_layout_entries() -> Vec<wgpu::BindGroupLayoutEntry> {
    let storage_entry = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
            count: None,
        },
        storage_entry(3),
        storage_entry(4),
        storage_entry(16),
        storage_entry(17),
        storage_entry(18),
        wgpu::BindGroupLayoutEntry {
            binding: 19,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        skinning::posed_entry(),
        foliage_entry(),
        wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 7,
            // Water's corners read the scene's copy here too (how deep it
            // is under them), so its waves calm at the shore.
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 10,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::Cube,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 11,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::CubeArray,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 14,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 15,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
    ]
}

/// The scene's samples per pixel, whether FXAA needs its target, whether
/// ambient occlusion needs the prepass's and its own (contact shadows and
/// volumetric fog need only the prepass's depth, and screen-space outlines,
/// the draw list's `edges`, its depth and normals), and whether depth of
/// field (`wants_depth_of_field`, if its setting allows) needs its target
/// and the prepass's depth, and which motion targets TAA and motion blur
/// (`wants_motion_blur`, if its setting allows) need, with the prepass's.
fn targets_used(
    graphics: &EffectiveGraphics,
    edges: bool,
    wants_depth_of_field: bool,
    wants_motion_blur: bool,
) -> (u32, bool, bool, bool, temporal::TemporalUse) {
    let anti_aliasing = graphics.anti_aliasing;
    let depth_of_field =
        wants_depth_of_field && graphics.depth_of_field != crate::DepthOfFieldQuality::Off;
    let temporal = temporal::TemporalUse::of(graphics, wants_motion_blur);
    (
        anti_aliasing.samples(),
        anti_aliasing == crate::AntiAliasing::Fxaa,
        graphics.ambient_occlusion != crate::AmbientOcclusion::Off
            || ray_ao::traced(graphics)
            || ray_reflections::traced(graphics)
            || screen_reflections::marched(graphics)
            || graphics.contact_shadows
            || graphics.volumetric_fog
            || graphics.ray_traced_shadows != crate::RayTracedShadows::Off
            || edges
            || depth_of_field
            || temporal.any(),
        depth_of_field,
        temporal,
    )
}

/// A 1×1 white occlusion texture: nothing hidden.
fn create_white_occlusion(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("no ambient occlusion"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OCCLUSION_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &[255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(1),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// The scene's size in pixels for an `output` of that size: scaled by the
/// render scale, at least 1, and at most what the GPU's textures can hold.
pub(super) fn scene_size(
    (width, height): (u32, u32),
    graphics: &EffectiveGraphics,
    capabilities: &Capabilities,
) -> (u32, u32) {
    let scaled = |pixels: u32| {
        ((pixels as f32 * graphics.render_scale).round() as u32)
            .clamp(1, capabilities.max_texture_size.max(1))
    };
    (scaled(width), scaled(height))
}

/// The glyphon bounds of one copy of `text`, `offset` pixels from it: its
/// box, moved with the copy so a shadow is cut where its letters are.
pub(super) fn text_bounds(text: &UiText, [dx, dy]: [f32; 2]) -> TextBounds {
    TextBounds {
        left: (text.position[0] + dx).floor() as i32,
        top: (text.position[1] + dy).floor() as i32,
        right: (text.position[0] + dx + text.size[0].max(0.0)).ceil() as i32,
        bottom: (text.position[1] + dy + text.size[1].max(0.0)).ceil() as i32,
    }
}

/// The shadow passes' bindings: the `frame` uniform, each view's matrix in
/// `cascades` (picked by a dynamic offset), and the frame's `posed` corners.
fn create_shadow_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    frame: &wgpu::Buffer,
    cascades: &wgpu::Buffer,
    (posed, foliage): (&wgpu::Buffer, &wgpu::Buffer),
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow pass"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: cascades,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<[[f32; 4]; 4]>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: skinning::POSED_BINDING,
                resource: posed.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: foliage::FOLIAGE_BINDING,
                resource: foliage.as_entire_binding(),
            },
        ],
    })
}

/// The plants' foliage table (`foliage.rs`), which the vertex shaders read
/// each corner's wind weights from.
fn foliage_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: foliage::FOLIAGE_BINDING,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// One step of drawing a shadow view: casters drawn into a layer (cleared
/// first, or over what it holds), or a lamp's kept still casters copied
/// into its layer (renderer/lamp_shadows.rs).
enum ShadowStep<'a> {
    Draw(
        &'a wgpu::TextureView,
        bool,
        &'a [(lighting::CasterGroup, Range<u32>)],
    ),
    Copy,
}

/// `AXCENITY_CUT_OUT_FILL` (set to anything) draws the cut-outs' depth fill
/// even where the scene's depth started from the prepass's, as before
/// 2026-11-28, for comparing.
fn cut_out_fill_forced() -> bool {
    static FORCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| std::env::var_os("AXCENITY_CUT_OUT_FILL").is_some())
}
