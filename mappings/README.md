# XIV MIDI 键位映射说明

## 文件位置

- **GUI版本**: 映射文件放在程序所在目录的 `mappings` 文件夹中
- **CLI版本**: 使用 `--mapping` 参数指定映射文件路径

## 内置映射

### Default FFXIV（默认）
- 覆盖3个八度（C3-C6，MIDI 48-84）
- 使用FFXIV性能键盘布局：`Q 2 W 3 E R 5 T 6 Y 7 U I`
- 映射规则：
  - **C3-B3（MIDI 48-59）**: Ctrl + 键
  - **C4-B4（MIDI 60-71）**: 无修饰符
  - **C5-B5（MIDI 72-84）**: Shift + 键

## 自定义映射

### 文件格式

映射文件使用JSON格式，结构如下：

```json
{
  "channel": 0,  // MIDI通道 (0-15，或null表示所有通道)
  "mappings": {
    "60": {  // MIDI音符号
      "on_press": [  // 按下时的动作序列
        {
          "Press": "Q"  // 按下Q键
        }
      ],
      "on_release": [  // 释放时的动作序列
        {
          "Release": "Q"  // 释放Q键
        }
      ]
    }
  }
}
```

### 支持的动作类型

1. **Press** - 按下按键
   ```json
   {"Press": "A"}
   ```

2. **Release** - 释放按键
   ```json
   {"Release": "A"}
   ```

3. **SetModifiers** - 设置修饰键状态
   ```json
   {
     "SetModifiers": {
       "shift": true,
       "ctrl": false,
       "alt": false
     }
   }
   ```

4. **Delay** - 延迟（毫秒）
   ```json
   {"Delay": 100}
   ```

### 支持的按键

- 字母键：A-Z
- 数字键：Num0-Num9（小键盘）、0-9（主键盘）
- 功能键：F1-F12
- 修饰键：Shift、Control、Alt、Meta
- 特殊键：Space、Enter、Escape、Tab、Backspace
- 方向键：Up、Down、Left、Right

### 示例

#### 简单映射
```json
{
  "channel": null,
  "mappings": {
    "60": {
      "on_press": [{"Press": "A"}],
      "on_release": [{"Release": "A"}]
    },
    "62": {
      "on_press": [{"Press": "B"}],
      "on_release": [{"Release": "B"}]
    }
  }
}
```

#### 带修饰键的映射
```json
{
  "channel": 0,
  "mappings": {
    "60": {
      "on_press": [
        {
          "SetModifiers": {
            "shift": false,
            "ctrl": true,
            "alt": false
          }
        },
        {"Press": "C"}
      ],
      "on_release": [
        {"Release": "C"},
        {
          "SetModifiers": {
            "shift": false,
            "ctrl": false,
            "alt": false
          }
        }
      ]
    }
  }
}
```

## 使用方法

### GUI版本

1. 启动程序后，会自动扫描 `mappings` 文件夹
2. 在 "Key Mapping" 下拉框中选择需要的映射
3. 未连接设备时，点击 "Apply Mapping" 应用映射
4. 已连接设备时，需要断开重连才能应用新映射
5. 点击 "Refresh Mappings" 重新扫描映射文件夹

### CLI版本

#### 使用默认映射
```bash
xiv-midi run --device "Your MIDI Device"
```

#### 使用自定义映射
```bash
xiv-midi run --device "Your MIDI Device" --mapping path/to/mapping.json
```

#### 生成默认配置文件
```bash
xiv-midi generate-config --output my_mapping.json
```

## 提示

- 映射文件名会显示在下拉列表中（不含.json扩展名）
- 建议使用有意义的文件名，如 `piano_layout.json`、`bard_songs.json` 等
- 可以基于生成的默认配置进行修改
- 修改映射文件后，点击 "Refresh Mappings" 即可重新加载
