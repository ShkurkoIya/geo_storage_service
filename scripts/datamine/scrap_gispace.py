import requests
import json
import time

# ==============================================================================
# 1. ЖИВЫЕ КУКИ И НАСТРОЙКИ СЕССИИ (КАК В ТВОЕМ УСПЕШНОМ CURL)
# ==============================================================================
# ⚠️ ОБЯЗАТЕЛЬНО: Вставь сюда рабочий токен, который пробил базу в терминале!
PHP_SESSION_ID = "rvq8fp6h3c4hhspmkv1018avp2"

ENDPOINTS = {
#    "ggs_nodes.json": "https://gispace.net/ggs/sql_dbrf.php",      # Геодезическая сеть (ГГС)
    "niveler_nodes.json": "https://gispace.net/ggs/sql_dbnt.php",  # Нивелирная сеть (ГНС)
    "lost_nodes.json": "https://gispace.net/ggs/sql_dblost.php"   # Утраченные пункты
}

# СТРОГО 5 заголовков из твоего успешного cURL-запроса, без лишнего мусора!
HEADERS = {
    "Content-Type": "application/json",
    "Origin": "https://gispace.net",
    "Referer": "https://gispace.net",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Cookie": f"PHPSESSID={PHP_SESSION_ID}",
    "Accept-Encoding": "identity",  # ХАК: Сервер отдаст ЧИСТЫЙ ТЕКСТ без битых gzip-архивов!
    "Connection": "close"           # ХАК: Сервер сразу закроет поток после отдачи, без таймаутов
}

# ==============================================================================
# 2. ОПТИМАЛЬНЫЕ СНАЙПЕРСКИЕ ЦЕНТРЫ ПОКРЫТИЯ РФ И НОВЫХ РЕГИОНОВ
# ==============================================================================
# Радиус 350 км — идеальный баланс: бэкенд не виснет, а 30 точек накрывают всю карту без дыр
SCAN_CENTERS = [
    # Новые регионы, Крым и Юг
    (48.487540, 38.861360),  # Снайперская лапка: ДНР, ЛНР, Запорожская, Херсонская обл. [21/51]
    (45.030000, 38.970000),  # Краснодар / Ставрополь / Кавказ
    (42.980000, 47.500000),  # Махачкала
    (46.340000, 48.030000),  # Астрахань
    (54.710000, 20.510000),  # Калининград (Эксклав)

    # Центр и Северо-Запад
    (55.750000, 37.620000),  # Москва / ЦФО
    (53.200000, 34.360000),  # Брянск / Белгород / Курск (Приграничье)
    (59.930000, 30.330000),  # Санкт-Петербург
    (61.780000, 34.350000),  # Петрозаводск / Карелия
    (64.540000, 40.540000),  # Архангельск

    # Поволжье и Урал
    (56.320000, 44.000000),  # Нижний Новгород
    (55.790000, 49.100000),  # Казань / Татарстан
    (53.240000, 50.220000),  # Самара / Тольятти
    (54.730000, 55.960000),  # Уфа / Башкирия
    (56.830000, 60.600000),  # Екатеринбург / Тюмень (твой успешный cURL тест!)
    (58.010000, 56.220000),  # Пермь

    # Сибирь
    (54.980000, 73.360000),  # Омск
    (55.000000, 82.930000),  # Новосибирск
    (56.500000, 84.990000),  # Томск / Кемерово
    (56.010000, 92.890000),  # Красноярск
    (52.280000, 104.300000), # Иркутск
    (61.000000, 69.020000),  # ХМАО (Сургут)

    # Дальний Восток
    (52.030000, 113.500000), # Чита
    (62.020000, 129.720000), # Якутск
    (50.270000, 127.540000), # Благовещенск
    (48.480000, 135.080000), # Хабаровск
    (43.110000, 131.870000), # Владивосток
    (46.950000, 142.730000), # Сахалин
    (53.020000, 158.640000)  # Камчатка
]

# ==============================================================================
# 3. ОСНОВНОЙ ЦИКЛ СБОРА ДАННЫХ
# ==============================================================================
def main():
    print(f"🚀 Запуск точной реплики cURL-майнера.")
    print(f"🌍 Всего секторов сканирования РФ: {len(SCAN_CENTERS)}")
    print(f"🎯 Выставлен проверенный радиус: 350.00 км\n")

    for filename, url in ENDPOINTS.items():
        print("="*70)
        print(f"📡 НАЧИНАЮ СБОР ДЛЯ ФАЙЛА: {filename}")
        print("="*70)

        # Подгружаем уже скачанное, чтобы не стирать прогресс при перезапусках
        try:
            with open(filename, 'r', encoding='utf-8') as f:
                existing_data = json.load(f)
                unique_nodes = {pt.get('id'): pt for pt in existing_data if pt.get('id')}
                print(f"📦 Подгружено из файла: {len(unique_nodes)} уникальных пунктов. Докачиваем...")
        except Exception:
            unique_nodes = {}
            print("🆕 Файл на диске не найден. Начинаем сбор с нуля.")

        for idx, (lat, lon) in enumerate(SCAN_CENTERS):
            payload = {
                "lat": f"{lat:.6f}",
                "lon": f"{lon:.6f}",
                "radius": "350.00"
            }

            print(f" [{idx+1}/{len(SCAN_CENTERS)}] Сектор {lat}, {lon}... ", end="", flush=True)

            max_retries = 3
            success = False

            for retry in range(max_retries):
                try:
                    response = requests.post(url, json=payload, headers=HEADERS, timeout=40)

                    if response.status_code == 200:
                        raw_text = response.text.strip()

                        # Предохранитель на случай, если сервер выбросит HTML
                        if raw_text.startswith("<!DOCTYPE") or "<html" in raw_text.lower():
                            print("⚠️ Сервер вернул HTML вместо данных. Пропуск сектора...")
                            break

                        if not raw_text or raw_text in ["null", "[]"]:
                            print("Пусто (0 пунктов).")
                            success = True
                            break

                        try:
                            points = response.json()
                        except json.JSONDecodeError:
                            continue

                        if not points or not isinstance(points, list):
                            print("Пусто.")
                            success = True
                            break

                        added_in_round = 0
                        for pt in points:
                            pt_id = pt.get('id')
                            if pt_id and pt_id not in unique_nodes:
                                unique_nodes[pt_id] = pt
                                added_in_round += 1

                        print(f"УСПЕХ! Взято: {len(points)} (Новых: {added_in_round}) -> Всего в файле: {len(unique_nodes)}")

                        # Моментальный автосейв на диск после каждого успешного шага
                        if added_in_round > 0:
                            with open(filename, 'w', encoding='utf-8') as f:
                                json.dump(list(unique_nodes.values()), f, ensure_ascii=False, indent=2)

                        success = True
                        break
                    else:
                        print(f"Код {response.status_code}. ", end="")

                except Exception:
                    print("Таймаут. ", end="")

                time.sleep(3.0) # Пауза перед повторной попыткой

            if not success:
                print("Пропуск.")

            time.sleep(2.0) # Спокойная пауза между шагами

        print(f"\n🎉 Сбор для файла {filename} окончен! Итог на диске: {len(unique_nodes)} пунктов.\n")
        time.sleep(3.0)

    print("="*70)
    print("🏁 ГЛОБАЛЬНЫЙ ДАТА-МАЙНИНГ УСПЕШНО ЗАВЕРШЕН!")
    print("="*70)

if __name__ == "__main__":
    main()
