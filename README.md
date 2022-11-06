# lemola_osv2
lemola_os (v1) ではuefi-rsを使わずに行き詰まってしまったので、とりあえずkernelをbootするところまでサクッといきたい

# 起動方法

## 1. Rust を install する。
linux系の場合、Rustツールチェインのインストールは次を実行することでできる。
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
参考 : [https://www.rust-lang.org/ja/tools/install](https://www.rust-lang.org/ja/tools/install)

環境変数の設定のためにシェルを再起動したりするのを忘れないようにしましょう。

## 2. 必要な package を install する。

```bash
./setup.sh
```

## 2. 動かす
```bash
make run
```
