@echo off

cargo build --release

copy /v /y .\target\release\*.exe .