# Wallpaper Engine for macOS

## 关于

这是一个 Wallpaper Engine 在 macOS 上的开源实现，使用 C++、Objective C++、Rust、Swift 混合实现。渲染引擎使用魔改后的 open-wallpaper-engine，额外支持了很多功能。

> ⚠️ 本项目是一个学习项目，一个对原始 Wallpaper Engine 的开源实现，显然不是所有壁纸都能完美使用的。我精力有限，因此随缘更新，但欢迎提交 PR。由于相较原版 open-wallpaper-engine 改动很多，因此不打算提交到上游（而且我特别讨厌C++，不打算改了，能用就行）

![单屏音频响应](assets/截屏2026-05-19%2001.07.30.webp)
![多屏音频响应](assets/截屏2026-05-19%2001.10.55.webp)
![GUI](assets/截屏2026-05-19%2001.08.49.webp)

## 兼容性

本程序不支持 Intel Mac，只支持搭载了 Apple Silicon 芯片的 Mac。由于使用了 macOS SDK 26，建议在 macOS 26 上使用，其他版本未经测试。

## 功能特性

渲染引擎相较原版（提交哈希 4145cfc90ed348d219a843d8549668b1ac9d3b4f），有以下功能差异：

- [x] 使用 Apple vDSP 加速的音频 FFT 变换加速
    - [x] 音频响应
- [x] 使用 FFmpeg 和 Apple VideoToolBox 实现的音频和视频管线
    - [x] 音频解码硬件加速
    - [x] 视频纹理
- [x] 参考 linux-wallpaperengine 实现的脚本引擎
    - [x] 部分支持 SceneScript
- [x] 使用 Rust 完全重新实现的着色器管线，具有更好的兼容性和更快的代码生成速度
- [x] 支持用户自定义属性、缩放系数

以及可能还有一些我不记得的修改

## 使用教程

首先你需要有一份正版的 Wallpaper Engine 副本，本程序需要使用原版 Wallpaper Engine 的资源，程序不认除了 Steam 以外的安装路径。

### 1.获取壁纸和Assets

#### 方法一：从Windows版本中复制

打开Windows版本的Wallpaper Engine，随便找一个壁纸，右键，在资源管理器中显示，返回到上一层目录（或者直接前往```Steam游戏安装目录\workshop\content\431960```(下载的)或者```Steam游戏安装目录\common\wallpaper_engine\projects\myprojects```(自制的)），将里面的所有文件夹复制（或者只复制自己想要的壁纸）到macOS上面的任意目录

再复制```Steam游戏安装目录\common\wallpaper_engine\assets```中的内容，到另一个目录（不要和壁纸放一起）

#### 方法二：使用SteamCMD下载

**安装 SteamCMD**

- 使用 [Homebrew](https://brew.sh/zh-cn/) 安装

```shell
brew install steamcmd
```

- 从 [官网](https://developer.valvesoftware.com/wiki/SteamCMD) 下载

```shell
mkdir ~/Steam && cd ~/Steam
curl -sqL "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_osx.tar.gz" | tar zxvf -

# 添加到环境变量
echo "export PATH=$PATH:$HOME/Steam" >> ~/.zshrc
source ~/.zshrc
```

**打开 SteamCMD 并登录**

```shell
> steamcmd

Steam Console Client (c) Valve Corporation - version 1778284286
-- type 'quit' to exit --
Loading Steam API...OK

Steam> login 用户名 密码 <可选的Steam令牌验证码>
Logging in using username/password.
Logging in user 'UserName' to Steam Public...This account is protected by a Steam Guard mobile authenticator.
Please confirm the login in the Steam Mobile app on your phone.
```

如果没有给验证码，你就需要去你的 Steam APP 上批准登录

1.3.2 安装 Wallpaper Engine

```shell
# 设置平台类型为 Windows
Steam> @sSteamCmdForcePlatformType windows

# 安装 Wallpaper Engine
Steam> app_update 431960 validate
```

安装过程可能会很长，因为会连带创意工坊内容一起下载下来，安装完成后后续的更新、创意工坊内容的下载可以使用 Steam 客户端进行

### 2. 安装 Wallpaper Engine for macOS

从[发布页面](https://github.com/bigsaltyfishes/wallpaper-engine-for-macos/releases)下载最新版本的程序，解压后复制到 `应用程序` 文件夹即可。由于程序没有签名，首次运行会被阻止，你需要在 系统设置->安全性与隐私->安全性 中点击`仍要运行`批准执行。

### 3. 使用

#### 3.1 设置壁纸

如果你是采用的方法一获取的壁纸和Assets，请前往设置调整壁纸和Assets目录
如果采用的是方法二则可以忽略（因为这是默认值）

如果你只是在主显示器上用，那就是开箱即用的，直接去壁纸页面选择壁纸、在显示器页面中展开`Primary`显示器的设置，点击启用，然后应用即可。副屏需要你在设置页面中启用显示器才能设置壁纸。考虑到 MacBook Air 没有散热，不建议 MacBook Air 多屏启用场景类型壁纸。

#### 3.2 音频响应

在壁纸页面选择壁纸，然后展开通用设置，启用音频响应即可，程序会请求系统录音权限，允许即可。

#### 3.3 16:10 屏幕支持

程序支持自定义缩放系数，在壁纸页面选择壁纸、展开显示器设置、展开对应显示器的设置，选择一个合适的缩放模式，再按需调整缩放系数。

缩放模式算法：
1. 无：壁纸里面设置了多大分辨率就按多大渲染，超出屏幕范围会直接裁剪
2. 拉伸：会把壁纸暴力拉倒屏幕分辨率，可能会变形
3. 匹配：根据壁纸分辨率X、Y中离屏幕分辨率最近的一侧进行缩放，该模式为等比缩放
4. 填充：根据壁纸分辨率X、Y中离屏幕分辨率最远的一侧进行缩放，该模式为等比缩放

#### 3.4 镜像壁纸

在设置中调整对应显示器模式为镜像即可

需要注意主显示器壁纸是设置给主显示器的，主显示器不按型号区别，只认系统汇报的那个显示器，因此镜像到主显示器时，壁纸不会随主显示器变化而变化。

#### 4. 日志记录

程序默认只记录 INFO 及更低级别的日志，你可以通过环境变量`WALLPAPER_ENGINE_LOG_LEVEL`调整这一级别，支持`trace`, `debug`, `info`, `error`, `warning`, `off`多个级别(级别逐级递减)。

## 编译程序

本程序使用 Nix 作为构建系统，大多数库都由 Nix 直接提供。但由于使用了 Swift 的一些专有库，编译前你需要在系统中安装 Xcode，版本要求 Xcode 26 以上。

```shell
# 克隆本仓库
git clone https://github.com/bigsaltyfishes/wallpaper-engine-for-macos --recursive

# 编译
cd wallpaper-engine-for-macos
nix build
```

## 开源协议

本程序在 GPL-2.0 协议下开源

## 参考项目

- 渲染引擎 [waywallen/open-wallpaper-engine](https://github.com/waywallen/open-wallpaper-engine)
- 渲染管线/着色器处理器/视频纹理/脚本引擎 [Almamu/linux-wallpaperengine](https://github.com/Almamu/linux-wallpaperengine)
- 程序图标 [Unayung/wallpaper-engine-mac](https://github.com/Unayung/wallpaper-engine-mac)
