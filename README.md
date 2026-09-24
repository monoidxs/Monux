# Monux

Минимальный Linux CLI на Rust. Цель: Arch в WSL2 и bare-metal Atom N450 / 2 ГБ.
Без внешних Rust-зависимостей, async runtime, демонов и обязательной LLM.

## Структура

- `crates/mono-cli`: аргументы, вывод, код завершения; бинарник `mono`.
- `crates/mono-core`: типы, trait Platform и capabilities; без системных команд.
- `crates/mono-platform-arch`: Linux/WSL status, pacman и iproute2.

Поток вызова: CLI → capability → Platform → Arch backend.
WSL — вариант среды Linux, а не отдельный менеджер пакетов.

## Работа в WSL / Arch

```bash
cd /root/monux/mono
export EDITOR=micro
micro crates/mono-cli/src/main.rs
cargo fmt --all --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo run --offline -- status
cargo run --offline -- network
cargo build --release --offline
# Следующая команда от root, либо через sudo:
install -m755 target/release/mono /usr/local/bin/mono
mono status
mono network
mono install nmap
```

Если отсутствуют micro или ip: `pacman -S --needed micro iproute2`.
Установка пакетов требует root, использует интерактивный pacman и не обновляет базы отдельно.
Полное обновление Arch выполняется отдельно через `pacman -Syu`.
`mono network` только читает адреса и IPv4/IPv6-маршруты; сеть не изменяет.

## Следующий шаг

Разделить network на `network status` и явный `network check <host>`.
Добавить структурированные интерфейсы/маршруты в core, затем диагностику DNS
и доступности с таймаутами. Отсутствие default route само по себе не считать
доказательством отсутствия интернета. Изменение настроек сети — отдельная capability.

## Слабое железо

Cargo собирает одним процессом. Release оптимизирован по размеру, без LTO;
нет `target-cpu=native`, чтобы не привязать сборку к современному процессору WSL.
На Atom необходимо отдельно проверить запуск, RAM и совместимость системных библиотек.
Размер и скорость на реальном Atom пока не измерялись.

## Сетевая диагностика

```bash
mono network status
mono network check archlinux.org
mono network check 1.1.1.1 443
mono network check ::1 22
```

Порт по умолчанию 443. Принимаются имя хоста или IP без URL; порт отдельным аргументом.
DNS: системный resolver, ожидание до 3 секунд. Для IP DNS пропускается.
TCP: общий бюджет 5 секунд, до 1 секунды на адрес, до первого успеха.
Код завершения 0 при успешном соединении, 1 при ошибке или таймауте.
Это проверка TCP, не TLS/HTTP и не доказательство доступности всего интернета.
Проверка не требует root и не меняет настройки сети.
Resolver работает в отдельном потоке: при таймауте ожидание прекращается,
но системный DNS-вызов не отменяется; процесс одноразового CLI завершает этот поток при выходе.
Для будущего долгоживущего агента потребуется отменяемый resolver или отдельный процесс.
Следующий этап: структурированные интерфейсы/маршруты и диагностика TLS/HTTP.
