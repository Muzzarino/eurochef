use std::{
    fs::File,
    io::{Seek, Write},
    path::Path,
};

use anyhow::Context;
use eurochef_edb::{
    binrw::{BinReaderExt, Endian},
    common::{EXVector2, EXVector3},
    entity::EXGeoBaseEntity,
    header::EXGeoHeader,
    versions::Platform,
};
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};

use crate::{edb::TICK_STRINGS, PlatformArg};

pub fn execute_command(
    filename: String,
    platform: Option<PlatformArg>,
    output_folder: Option<String>,
) -> anyhow::Result<()> {
    let output_folder = output_folder.unwrap_or(format!(
        "./entities/{}/",
        Path::new(&filename)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    ));
    let output_folder = Path::new(&output_folder);

    let mut file = File::open(&filename)?;
    let endian = if file.read_ne::<u8>().unwrap() == 0x47 {
        Endian::Big
    } else {
        Endian::Little
    };
    file.seek(std::io::SeekFrom::Start(0))?;

    let header = file
        .read_type::<EXGeoHeader>(endian)
        .expect("Failed to read header");

    let platform = platform
        .map(|p| p.into())
        .or(Platform::from_path(&filename))
        .expect("Failed to detect platform");

    if platform != Platform::Pc && platform != Platform::Xbox {
        anyhow::bail!("Entity extraction is only supported for PC and Xbox (for now)")
    }

    println!("Selected platform {platform:?}");

    let pb = ProgressBar::new(header.entity_list.data.len() as u64)
        .with_finish(indicatif::ProgressFinish::AndLeave);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {msg} ({pos}/{len})",
        )
        .unwrap()
        .progress_chars("##-")
        .tick_chars(&TICK_STRINGS),
    );
    pb.set_message("Extracting entities");

    std::fs::create_dir_all(output_folder)?;
    for e in header.entity_list.data.iter().progress_with(pb) {
        file.seek(std::io::SeekFrom::Start(e.common.address as u64))?;

        let ent = file
            .read_type_args::<EXGeoBaseEntity>(endian, (header.version,))
            .context("Failed to read entity")?;

        let esplit = ent.split_entity.as_ref();
        let nents = if ent.object_type == 1537 {
            vec![ent.normal_entity.as_ref().unwrap()]
        } else {
            esplit
                .unwrap()
                .entities
                .iter()
                .map(|r| r.data.normal_entity.as_ref().unwrap())
                .collect()
        };

        let mut vertex_data = vec![];
        let mut faces: Vec<(u32, u32, u32)> = vec![];
        let mut index_offset = 0;

        for nent in nents {
            file.seek(std::io::SeekFrom::Start(nent.vertex_data.offset_absolute()))?;
            for _ in 0..nent.vertex_count {
                match header.version {
                    252 | 250 | 240 => {
                        let d = file.read_type::<(EXVector3, u32, EXVector2)>(endian)?;
                        vertex_data.push((d.0, [0f32, 0f32, 0f32], d.2));
                    }
                    _ => {
                        vertex_data
                            .push(file.read_type::<(EXVector3, EXVector3, EXVector2)>(endian)?);
                    }
                }
            }

            file.seek(std::io::SeekFrom::Start(nent.index_data.offset_absolute()))?;
            let indices: Vec<u16> = (0..nent.index_count)
                .map(|_| file.read_type(endian).unwrap())
                .collect();

            let mut tristrips: Vec<(u32, i32)> = vec![];
            for i in 0..nent.tristrip_count {
                if header.version <= 252 {
                    file.seek(std::io::SeekFrom::Start(
                        nent.tristrip_data.offset_absolute() + i as u64 * 20,
                    ))?;
                } else {
                    file.seek(std::io::SeekFrom::Start(
                        nent.tristrip_data.offset_absolute() + i as u64 * 16,
                    ))?;
                }

                tristrips.push(file.read_type(endian)?);
            }

            let mut index_offset_local = 0;
            for (tricount, _texture) in tristrips {
                if tricount < 2 {
                    // panic!("Invalid tristrips found with only {tricount} indices")
                    continue;
                }
                // println!("{} / {}", tricount, indices.len());
                for i in (index_offset_local as usize)..(index_offset_local + tricount) as usize {
                    if (i - index_offset_local as usize) % 2 == 0 {
                        faces.push((
                            index_offset + indices[i] as u32,
                            index_offset + indices[i + 1] as u32,
                            index_offset + indices[i + 2] as u32,
                        ))
                    } else {
                        faces.push((
                            index_offset + indices[i + 2] as u32,
                            index_offset + indices[i + 1] as u32,
                            index_offset + indices[i] as u32,
                        ))
                    }
                }

                index_offset_local += tricount;
            }

            index_offset = vertex_data.len() as u32;
        }

        let mut outbuf = vec![];
        writeln!(&mut outbuf, "o obj_{:x}", e.common.hashcode)?;
        for (xyz, normal, uv) in vertex_data {
            writeln!(&mut outbuf, "v {} {} {}", -xyz[0], xyz[1], xyz[2])?;
            writeln!(&mut outbuf, "vn {} {} {}", normal[0], normal[1], normal[2])?;
            writeln!(&mut outbuf, "vt {} {}", uv[0], 1. - uv[1])?;
        }

        for (v0, v1, v2) in faces {
            // Skip face if it's a degenerate
            if v0 == v1 || v1 == v2 || v2 == v0 {
                continue;
            }

            writeln!(
                &mut outbuf,
                "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}",
                v0 + 1,
                v1 + 1,
                v2 + 1
            )?;
        }

        let mut outfile = File::create(output_folder.join(format!("{:x}.obj", e.common.hashcode)))?;
        outfile.write_all(&outbuf)?;
    }

    Ok(())
}
