use crate::models::reference::{GgsNodeDomain, LostNodeDomain, NivelerNodeDomain};
use crate::storage::repository::GeoRepository;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub struct GgsImportHandler;

impl GgsImportHandler {
    /// Импорт пунктов ГГС из JSON
    pub async fn import_ggs<P: AsRef<Path>>(
        repo: &GeoRepository<'_>,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        println!(
            "Модуль Импорта: Чтение гео-матрицы ГГС [{:?}]...",
            path_ref.file_name().unwrap()
        );

        let file = File::open(path_ref)?;
        let reader = BufReader::new(file);

        let nodes: Vec<GgsNodeDomain> = serde_json::from_reader(reader)?;
        let mut count = 0;

        for node in &nodes {
            repo.insert_ggs_node(node).await?;
            count += 1;
        }

        println!(
            "Успешно импортировано и валидировано {} пунктов ГГС.",
            count
        );
        Ok(())
    }

    /// Импорт утраченных пунктов из JSON
    pub async fn import_lost<P: AsRef<Path>>(
        repo: &GeoRepository<'_>,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        println!(
            "🗜️  Модуль Импорта: Сканирование карантина брака [{:?}]...",
            path_ref.file_name().unwrap()
        );

        let file = File::open(path_ref)?;
        let reader = BufReader::new(file);

        let nodes: Vec<LostNodeDomain> = serde_json::from_reader(reader)?;
        let mut count = 0;

        for node in &nodes {
            repo.insert_lost_node(node).await?;
            count += 1;
        }

        println!(
            "Успешно загружено в базу {} поврежденных пунктов lost_nodes.",
            count
        );
        Ok(())
    }

    /// Импорт нивелирных сетей из JSON
    pub async fn import_niveler<P: AsRef<Path>>(
        repo: &GeoRepository<'_>,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path_ref = path.as_ref();
        println!(
            "🗜️  Модуль Импорта: Чтение высотных эталонов ГНС [{:?}]...",
            path_ref.file_name().unwrap()
        );

        let file = File::open(path_ref)?;
        let reader = BufReader::new(file);

        let nodes: Vec<NivelerNodeDomain> = serde_json::from_reader(reader)?;
        let mut count = 0;

        for node in &nodes {
            repo.insert_niveler_node(node).await?;
            count += 1;
        }

        println!("Успешно загружено {} нивелирных высотных реперов.", count);
        Ok(())
    }
}
