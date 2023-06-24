use std::{
    io::{Read, Seek},
    sync::Arc,
};

use anyhow::Context;

use eurochef_edb::{
    binrw::{BinReaderExt, Endian},
    entity::{EXGeoEntity, EXGeoMapZoneEntity},
    header::EXGeoHeader,
    map::{EXGeoBaseDatum, EXGeoMap, EXGeoPlacement},
    versions::Platform,
};
use eurochef_shared::{textures::UXGeoTexture, IdentifiableResult};
use glam::Vec3;
use nohash_hasher::IntMap;

use crate::{
    entities::{EntityListPanel, ProcessedEntityMesh},
    entity_frame::RenderableTexture,
    map_frame::MapFrame,
    render::viewer::CameraType,
};

pub struct MapViewerPanel {
    _gl: Arc<glow::Context>,

    maps: Vec<ProcessedMap>,
    _entities: Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
    _ref_entities: Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
    _textures: Vec<RenderableTexture>,

    // TODO(cohae): Replace so we can do funky stuff
    frame: MapFrame,
}

#[derive(Clone)]
pub struct ProcessedMap {
    pub hashcode: u32,
    pub mapzone_entities: Vec<EXGeoMapZoneEntity>,
    pub placements: Vec<EXGeoPlacement>,
    pub triggers: Vec<ProcessedTrigger>,
    pub trigger_collisions: Vec<EXGeoBaseDatum>,
}

#[derive(Clone)]
pub struct ProcessedTrigger {
    pub link_ref: i32,

    pub ttype: u32,
    pub tsubtype: Option<u32>,

    pub debug: u16,
    pub game_flags: u32,
    pub trig_flags: u32,
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    pub data: Vec<Option<u32>>,
    pub links: Vec<i32>,
    pub engine_data: Vec<Option<u32>>,

    /// Every trigger that links to this one
    pub incoming_links: Vec<i32>,
}

impl MapViewerPanel {
    pub fn new(
        _ctx: &egui::Context,
        gl: Arc<glow::Context>,
        maps: Vec<ProcessedMap>,
        entities: Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
        ref_entities: Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
        textures: &[IdentifiableResult<UXGeoTexture>],
        platform: Platform,
        hashcodes: Arc<IntMap<u32, String>>,
    ) -> Self {
        let textures = EntityListPanel::load_textures(&gl, textures);
        MapViewerPanel {
            frame: Self::load_map_meshes(
                gl.clone(),
                &maps,
                &ref_entities,
                &entities,
                &textures,
                platform,
                hashcodes.clone(),
            ),
            _textures: textures,
            _gl: gl,
            maps,
            _entities: entities,
            _ref_entities: ref_entities,
        }
    }

    fn load_map_meshes(
        gl: Arc<glow::Context>,
        maps: &[ProcessedMap],
        ref_entities: &Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
        entities: &Vec<IdentifiableResult<(EXGeoEntity, ProcessedEntityMesh)>>,
        textures: &[RenderableTexture],
        platform: Platform,
        hashcodes: Arc<IntMap<u32, String>>,
    ) -> MapFrame {
        let mut map_entities = vec![];

        // FIXME(cohae): Map picking is a bit dirty at the moment
        for map in maps.iter() {
            for v in &map.mapzone_entities {
                if let Some(Ok((_, e))) = &ref_entities
                    .iter()
                    .find(|ir| ir.hashcode == v.entity_refptr)
                    .map(|v| v.data.as_ref())
                {
                    map_entities.push((map.hashcode, e));
                } else {
                    error!(
                        "Couldn't find ref entity #{} for mapzone entity!",
                        v.entity_refptr
                    );
                }
            }
        }

        let ef = MapFrame::new(gl, &map_entities, textures, entities, platform, hashcodes);
        ef.viewer
            .lock()
            .map(|mut v| {
                v.selected_camera = CameraType::Fly;
                v.show_grid = false;
            })
            .unwrap();

        ef
    }

    pub fn show(&mut self, context: &egui::Context, ui: &mut egui::Ui) -> anyhow::Result<()> {
        self.frame.show(ui, context, &self.maps)
    }
}

// TODO(cohae): EdbFile struct so we dont have to read endianness separately
pub fn read_from_file<R: Read + Seek>(reader: &mut R, platform: Platform) -> Vec<ProcessedMap> {
    reader.seek(std::io::SeekFrom::Start(0)).ok();
    let endian = if reader.read_ne::<u8>().unwrap() == 0x47 {
        Endian::Big
    } else {
        Endian::Little
    };
    reader.seek(std::io::SeekFrom::Start(0)).unwrap();

    let header = reader
        .read_type::<EXGeoHeader>(endian)
        .expect("Failed to read header");

    let mut maps = vec![];
    for m in header.map_list.iter() {
        reader
            .seek(std::io::SeekFrom::Start(m.address as u64))
            .unwrap();

        let xmap = reader
            .read_type_args::<EXGeoMap>(endian, (header.version,))
            .context("Failed to read map")
            .unwrap();

        let mut map = ProcessedMap {
            hashcode: m.hashcode,
            mapzone_entities: vec![],
            placements: xmap.placements.data().clone(),
            triggers: vec![],
            trigger_collisions: xmap.trigger_header.trigger_collisions.0.clone(),
        };

        for z in &xmap.zones {
            let entity_offset = header.refpointer_list[z.entity_refptr as usize].address;
            reader
                .seek(std::io::SeekFrom::Start(entity_offset as u64))
                .context("Mapzone refptr pointer to a non-entity object!")
                .unwrap();

            let ent = reader
                .read_type_args::<EXGeoEntity>(endian, (header.version, platform))
                .unwrap();

            if let EXGeoEntity::MapZone(mapzone) = ent {
                map.mapzone_entities.push(mapzone);
            } else {
                error!("Refptr entity does not have a mapzone entity!");
                // Result::<()>::Err(anyhow::anyhow!(
                //     "Refptr entity does not have a mapzone entity!"
                // ))
                // .unwrap();
            }
        }

        for t in xmap.trigger_header.triggers.iter() {
            let trig = &t.trigger;
            let (ttype, tsubtype) = {
                let t = &xmap.trigger_header.trigger_types[trig.type_index as usize];

                (t.trig_type, t.trig_subtype)
            };

            let trigger = ProcessedTrigger {
                link_ref: t.link_ref,
                ttype,
                tsubtype: if tsubtype != 0 && tsubtype != 0x42000001 {
                    Some(tsubtype)
                } else {
                    None
                },
                debug: trig.debug,
                game_flags: trig.game_flags,
                trig_flags: trig.trig_flags,
                position: trig.position.into(),
                rotation: trig.rotation.into(),
                scale: trig.scale.into(),
                engine_data: trig.engine_data.to_vec(),
                data: trig.data.to_vec(),
                links: trig.links.to_vec(),
                incoming_links: vec![],
            };

            map.triggers.push(trigger);
        }

        for i in 0..map.triggers.len() {
            for ei in 0..map.triggers.len() {
                if i == ei {
                    continue;
                }

                if map.triggers[ei]
                    .links
                    .iter()
                    .find(|v| **v == i as i32)
                    .is_some()
                {
                    map.triggers[i].incoming_links.push(ei as i32);
                }
            }
        }

        maps.push(map);
    }

    maps
}
