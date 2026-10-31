-- Дропаем сначала представление, так как оно зависит от таблиц
DROP VIEW IF EXISTS science_degas.v_hydrogen_prime_targets;

-- Дропаем таблицы
DROP TABLE IF EXISTS science_degas.hydrogen_rings;
DROP TABLE IF EXISTS science_degas.gas_plumes;
DROP TABLE IF EXISTS science_degas.thermal_hotspots;

-- Удаляем саму схему
DROP SCHEMA IF EXISTS science_degas;
