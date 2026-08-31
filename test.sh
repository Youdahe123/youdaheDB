#!/bin/bash

RESET="\033[0m"
BOLD="\033[1m"
DIM="\033[2m"
GREEN="\033[32m"
RED="\033[31m"
YELLOW="\033[33m"
CYAN="\033[36m"
WHITE="\033[97m"

clear

echo ""
echo -e "${BOLD}${CYAN}"
echo "  ██╗   ██╗ ██████╗ ██╗   ██╗██████╗  █████╗ ██╗  ██╗███████╗██████╗ ██████╗ ██████╗ "
echo "  ╚██╗ ██╔╝██╔═══██╗██║   ██║██╔══██╗██╔══██╗██║  ██║██╔════╝██╔══██╗██╔══██╗██╔══██╗"
echo "   ╚████╔╝ ██║   ██║██║   ██║██║  ██║███████║███████║█████╗  ██║  ██║██████╔╝██████╔╝"
echo "    ╚██╔╝  ██║   ██║██║   ██║██║  ██║██╔══██║██╔══██║██╔══╝  ██║  ██║██╔══██╗██╔══██╗"
echo "     ██║   ╚██████╔╝╚██████╔╝██████╔╝██║  ██║██║  ██║███████╗██████╔╝██████╔╝██████╔╝"
echo "     ╚═╝    ╚═════╝  ╚═════╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚══════╝╚═════╝ ╚═════╝ ╚═════╝ "
echo -e "${RESET}"
echo -e "${DIM}  Distributed Database Engine — Test Suite${RESET}"
echo ""
echo -e "${DIM}  ─────────────────────────────────────────────────${RESET}"
echo ""

GITHUB_LINK="\e]8;;https://github.com/youdaheasfaw\e\\${BOLD}${WHITE}\e[4mYoudahe Asfaw\e[24m${RESET}\e]8;;\e\\"
LINKEDIN_LINK="\e]8;;https://www.linkedin.com/in/youdaheasfaw\e\\${DIM}LinkedIn\e[0m\e]8;;\e\\"

sleep 0.05
echo -e "  A distributed database engine built by $GITHUB_LINK  ${DIM}·  $LINKEDIN_LINK${RESET}"
echo ""
sleep 0.07
echo -e "  ${BOLD}What makes it efficient:${RESET}"
echo ""

blurbs=(
    "Write-Ahead Log turns every write into a sequential disk append — no random I/O"
    "LSM Tree batches writes in memory and flushes sorted to disk, maximizing throughput"
    "Bloom filters skip disk reads for keys that don't exist, cutting read latency"
    "Raft consensus replicates data across nodes without a single point of failure"
    "Consistent hashing distributes load evenly and minimizes reshuffling as nodes change"
    "Two-phase commit + OCC gives full ACID guarantees across shards with minimal locking"
)

for blurb in "${blurbs[@]}"; do
    sleep 0.1
    echo -e "  ${CYAN}·${RESET}  ${DIM}$blurb${RESET}"
done

echo ""
echo -e "${DIM}  ─────────────────────────────────────────────────${RESET}"
echo ""
echo -e "  ${BOLD}${WHITE}Commands${RESET}"
echo ""

commands=(
    "${CYAN}./test.sh${RESET}                   run this test suite"
    "${CYAN}cargo test${RESET}                  run all tests with full output"
    "${CYAN}cargo test wal${RESET}              run only the WAL layer tests"
    "${CYAN}cargo build${RESET}                 compile the project"
    "${CYAN}cargo build --release${RESET}       compile optimized build"
)

for cmd in "${commands[@]}"; do
    sleep 0.08
    echo -e "  ${DIM}›${RESET}  ${cmd}"
done

echo ""
echo -e "${DIM}  ─────────────────────────────────────────────────${RESET}"
echo ""

run_layer() {
    local name=$1
    local filter=$2
    local status=$3  # "live" or "pending"

    if [ "$status" = "pending" ]; then
        echo -e "  ${DIM}[ PENDING ]  $name${RESET}"
        return
    fi

    printf "  ${BOLD}${WHITE}[ RUNNING ]${RESET}  ${BOLD}$name${RESET}"

    output=$(cargo test "$filter" --quiet 2>&1)
    exit_code=$?

    if [ $exit_code -eq 0 ]; then
        passed=$(echo "$output" | grep -o '[0-9]* passed' | head -1)
        echo -e "\r  ${BOLD}${GREEN}[ PASS ]${RESET}     ${BOLD}$name${RESET}  ${DIM}$passed${RESET}"
    else
        echo -e "\r  ${BOLD}${RED}[ FAIL ]${RESET}     ${BOLD}$name${RESET}"
        echo ""
        echo "$output" | grep "FAILED\|thread\|panicked" | sed 's/^/    /'
        echo ""
    fi
}

echo -e "  ${BOLD}Layer Status${RESET}"
echo ""

run_layer "WAL (Write-Ahead Log)"          "wal::"              "live"
run_layer "MemTable"                        "memtable::"         "live"
run_layer "SSTable"                         "sstable::"          "pending"
run_layer "LSM Tree"                        "lsm::"              "pending"
run_layer "Raft Consensus"                  "raft::"             "pending"
run_layer "Sharding"                        "sharding::"         "pending"
run_layer "Transactions"                    "transaction::"      "pending"

echo ""
echo -e "${DIM}  ─────────────────────────────────────────────────${RESET}"
echo -e "  ${DIM}Run ${RESET}${CYAN}cargo test${RESET}${DIM} directly for full output${RESET}"
echo ""
