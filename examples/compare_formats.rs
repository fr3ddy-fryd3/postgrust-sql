/// Сравнение размеров JSON vs Binary форматов
/// Запустите тесты чтобы создать binary файлы, затем запустите этот пример
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════╗");
    println!("║  Сравнение форматов хранения          ║");
    println!("╚════════════════════════════════════════╝\n");

    // Проверяем JSON файл если есть
    if let Ok(metadata) = fs::metadata("data/main.json") {
        let json_size = metadata.len();
        println!("📄 JSON формат (старый):");
        println!("   Файл: data/main.json");
        println!("   Размер: {} bytes ({:.1} KB)", json_size, json_size as f64 / 1024.0);
    } else {
        println!("⚠  JSON файл не найден");
    }

    println!();

    // Проверяем Binary файл если есть
    if let Ok(metadata) = fs::metadata("data/main.db") {
        let binary_size = metadata.len();
        println!("💾 Binary формат (новый):");
        println!("   Файл: data/main.db");
        println!("   Размер: {} bytes ({:.1} KB)", binary_size, binary_size as f64 / 1024.0);
    } else {
        println!("⚠  Binary файл не найден");
        println!("   Запустите сервер или тесты чтобы создать binary snapshot");
    }

    println!();

    // Сравниваем если оба есть
    if let (Ok(json_meta), Ok(bin_meta)) = (
        fs::metadata("data/main.json"),
        fs::metadata("data/main.db")
    ) {
        let json_size = json_meta.len();
        let bin_size = bin_meta.len();
        let saved = json_size.saturating_sub(bin_size);
        let saved_percent = (saved as f64 / json_size as f64 * 100.0) as i64;

        println!("╔════════════════════════════════════════╗");
        println!("║          Результаты сравнения          ║");
        println!("╠════════════════════════════════════════╣");
        println!("║ Экономия:        {:>6} bytes        ║", saved);
        println!("║ Процент экономии: {:>3}%               ║", saved_percent);
        println!("║ Binary = {:.1}% от JSON              ║",
            100.0 - saved_percent as f64);
        println!("╚════════════════════════════════════════╝");
    }

    // Проверяем WAL файлы
    println!("\n📝 WAL файлы:");
    if let Ok(entries) = fs::read_dir("data/wal") {
        let mut total_size = 0u64;
        let mut count = 0;

        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                    let size = entry.metadata()?.len();
                    total_size += size;
                    count += 1;
                    println!("   {} - {} bytes",
                        path.file_name().unwrap().to_str().unwrap(), size);
                }
            }
        }

        if count > 0 {
            println!("\n   Всего WAL файлов: {}", count);
            println!("   Общий размер: {} bytes ({:.1} KB)",
                total_size, total_size as f64 / 1024.0);
        } else {
            println!("   (пусто)");
        }
    }

    Ok(())
}
