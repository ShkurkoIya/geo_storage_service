import requests
import json
import time
import math

# ==============================================================================
# 1. ЖИВЫЕ КУКИ И НАСТРОЙКИ СЕССИИ (КАК В ТВОЕМ УСПЕШНОМ CURL)
# ==============================================================================
# ⚠️ ОБЯЗАТЕЛЬНО: Вставь сюда рабочий токен из своего успешного cURL-теста
PHP_SESSION_ID = "rvq8fp6h3c4hhspmkv1018avp2"

ENDPOINTS = {
#    "ggs_nodes.json": "https://gispace.net/ggs/sql_dbrf.php",      # Геодезическая сеть (ГГС)
    "niveler_nodes.json": "https://gispace.net/ggs/sql_dbnt.php",  # Нивелирная сеть (ГНС)
    "lost_nodes.json": "https://gispace.net/ggs/sql_dblost.php"   # Утраченные пункты
}

# Строгая реплика твоего успешного cURL-запроса, которая отключает битые gzip архивы
HEADERS = {
    "Content-Type": "application/json",
    "Origin": "https://gispace.net",
    "Referer": "https://gispace.net",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Cookie": f"PHPSESSID={PHP_SESSION_ID}",
    "Accept-Encoding": "identity",
    "Connection": "close"
}

# ==============================================================================
# 2. МАТЕМАТИЧЕСКИ ИДЕАЛЬНАЯ СЕТКА ПОКРЫТИЯ РФ И НОВЫХ РЕГИОНОВ
# ==============================================================================
def generate_safe_grid(step_km=200):
    """
    Генерирует плотную сетку координат. Границы выставлены так, чтобы
    намертво зажать Новые регионы, Крым, Калининград и дойти до Чукотки.
    С шагом 200 км при радиусе 180 км получается идеальный ковер без слепых зон.
    """
    # Снайперские границы (Широта от ДНР/Крыма, Долгота от Калининграда)
    LAT_MIN, LAT_MAX = 43.0, 74.0
    LON_MIN, LON_MAX = 19.5, 180.0

    grid_points = []
    lat = LAT_MIN
    while lat <= LAT_MAX:
        lat_rad = math.radians(lat)
        km_per_lon = 111.32 * math.cos(lat_rad)

        # Коррекция шага долготы ближе к северу, чтобы не спамить точки на полюсе
        lon_step = step_km / km_per_lon if km_per_lon > 1 else 4.0
        lat_step = step_km / 111.1

        lon = LON_MIN
        while lon <= LON_MAX:
            # Отсекаем только глубокую чужую Европу, оставляя Калининград и Юг на месте
            if lat < 53.0 and lon < 31.5:
                if not (lat > 54.0 and lat < 56.0 and lon > 19.5 and lon < 23.0):
                    lon += lon_step
                    continue
            grid_points.append((lat, lon))
            lon += lon_step
        lat += lat_step

    return grid_points

# ==============================================================================
# 3. ОСНОВНОЙ ЦИКЛ СКАНИРОВАНИЯ И АВТОСОХРАНЕНИЯ
# ==============================================================================
def main():
    grid = generate_safe_grid(step_km=200)

    print(f"🚀 Запуск ОПТИМИЗИРОВАННОГО коврового майнинга всей площади РФ.")
    print(f"🌍 Всего секторов сканирования в сетке: {len(grid)}")
    print(f"🎯 Выставлен безопасный радиус поиска: 180.00 км (Защита от SQL LIMIT и падения PHP)\n")

    for filename, url in ENDPOINTS.items():
        print("="*80)
        print(f"📡 НАЧИНАЮ СБОР ДЛЯ ФАЙЛА: {filename}")
        print("="*80)

        # Интеллектуальная докачка: подгружаем старый прогресс, если файлы уже есть на диске
        try:
            with open(filename, 'r', encoding='utf-8') as f:
                existing_data = json.load(f)
                unique_nodes = {pt.get('id'): pt for pt in existing_data if pt.get('id')}
                print(f"📦 Подгружено из файла: {len(unique_nodes)} уникальных пунктов. Докачиваем...")
        except Exception:
            unique_nodes = {}
            print("🆕 Файл на диске не найден. Начинаем сбор с нуля.")

        for idx, (lat, lon) in enumerate(grid):
            payload = {
                "lat": f"{lat:.6f}",
                "lon": f"{lon:.6f}",
                "radius": "180.00"  # Наш идеальный сбалансированный радиус
            }

            print(f" [{idx+1}/{len(grid)}] Сектор {lat:.3f}, {lon:.3f}... ", end="", flush=True)

            max_retries = 3
            success = False

            for retry in range(max_retries):
                try:
                    response = requests.post(url, json=payload, headers=HEADERS, timeout=30)

                    if response.status_code == 200:
                        raw_text = response.text.strip()

                        # Если пустые морские точки или чужие границы выдают ошибку/HTML,
                        # просто пропускаем их, не останавливая скрипт
                        if raw_text.startswith("<!DOCTYPE") or "<html" in raw_text.lower():
                            print("Морской сектор/Пусто. ", end="")
                            success = True
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

                        print(f"УСПЕХ! Взято: {len(points)} (Новых: {added_in_round}) -> Всего: {len(unique_nodes)}")

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

                time.sleep(3.0) # Пауза перед ретраем

            if not success:
                print("Пропуск.")

            time.sleep(1.5) # Вежливая задержка между шагами для СУБД сайта

        print(f"\n🎉 Сбор для файла {filename} окончен! Итог на диске: {len(unique_nodes)} пунктов.\n")
        time.sleep(3.0)

    print("="*80)
    print("🏁 ГЛОБАЛЬНЫЙ ТОТАЛЬНЫЙ КОВЕР УСПЕШНО ЗАВЕРШЕН!")
    print("="*80)

if __name__ == "__main__":
    main()
