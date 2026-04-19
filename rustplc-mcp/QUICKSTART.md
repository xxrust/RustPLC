# RustPLC MCP Server - 蹇€熷紑濮嬫寚鍗?
## 鍓嶇疆鏉′欢

1. **Python 3.10+**
2. **RustPLC 缂栬瘧鍣ㄥ凡鏋勫缓**
   ```bash
   cd /path/to/rust_plc
   cargo build --release
   ```
3. **瀹夎 MCP 鍖?*
   ```bash
   pip install mcp
   ```

## 瀹夎姝ラ

### 1. 楠岃瘉瀹夎

```bash
cd rustplc-mcp
python test_server.py
```

搴旇鐪嬪埌锛?```
All tests passed! MCP server is ready.
```

### 2. 閰嶇疆 Claude Code

椤圭洰鏍圭洰褰曞凡鏈?`.mcp.json` 閰嶇疆鏂囦欢锛?
```json
{
  "mcpServers": {
    "rustplc": {
      "type": "stdio",
      "command": "python",
      "args": ["-m", "server"],
      "cwd": "${workspaceFolder}/rustplc-mcp",
      "env": {
        "RUSTPLC_PATH": "${workspaceFolder}/target/release/rust_plc"
      }
    }
  }
}
```

### 3. 閲嶅惎 Claude Code

閲嶅惎鍚?MCP 鏈嶅姟鍣ㄤ細鑷姩杩炴帴銆?
## 浣跨敤绀轰緥

### 绀轰緥 1锛氫粠闆剁敓鎴?PLC 绋嬪簭

```
浣? "甯垜鐢熸垚涓€涓弻缂搁『搴忓姩浣滅殑 PLC 绋嬪簭"

Claude Code 浼氾細
1. 璋冪敤 get_rustplc_skill_guide 鑾峰彇鐢熸垚鎸囧紩
2. 鎵ц鍥涢樁娈垫祦绋嬶紙.system.md 鈫?鐞嗚В宸ヨ壓 鈫?鎺ㄧ悊鎷撴墤 鈫?鎺ㄥ绾︽潫 鈫?鐢熸垚 DSL锛?3. 姣忎釜闃舵閮戒細绛夊緟浣犵‘璁?4. 鏈€缁堢敓鎴?.plc 鏂囦欢骞惰嚜鍔ㄨ皟鐢?validate_plc 楠岃瘉
5. 杩斿洖楠岃瘉閫氳繃鐨勫畬鏁存枃浠?```

### 绀轰緥 2锛氫娇鐢ㄦā鏉垮揩閫熺敓鎴?
```
浣? "/mcp__rustplc__two_cylinder_template button single"

Claude Code 浼氳繑鍥為濉厖鐨勫弻缂告ā鏉?```

### 绀轰緥 3锛氬弬鑰冪ず渚嬪涔?
```
浣? "鎴戞兂鐪嬬湅 PID 鎺у埗鎬庝箞鍐?

Claude Code 浼氾細
1. 璇诲彇 @rustplc://examples/force_override_demo.plc
2. 璇诲彇 @rustplc://docs/已实现/extern_function_mvp_spec.md
3. 缁撳悎涓よ€呯粰浣犺瑙?```

### 绀轰緥 4锛氶獙璇佺幇鏈変唬鐮?
```
浣? "甯垜楠岃瘉杩欎釜 .plc 鏂囦欢"
[绮樿创浠ｇ爜]

Claude Code 浼氾細
1. 璋冪敤 validate_plc 宸ュ叿
2. 杩斿洖璇︾粏鐨勯獙璇佹姤鍛?3. 濡傛灉澶辫触锛岀粰鍑轰慨澶嶅缓璁?```

## 鍙敤宸ュ叿

### Tools
- `get_rustplc_skill_guide` - 鑾峰彇缁熶竴鐨?RustPLC skill 鎸囧紩
- `validate_plc` - 楠岃瘉 .plc 鏂囦欢
- `compile_plc` - 缂栬瘧骞惰繑鍥?IR JSON

### Resources
- `@rustplc://examples/<filename>` - 璁块棶绀轰緥鏂囦欢
- `@rustplc://docs/<filename>` - 璁块棶鎶€鏈枃妗?- `@rustplc://skill/rustplc` - 璁块棶缁熶竴 skill 鎸囧紩

### Prompts
- `/mcp__rustplc__generate_plc_from_requirements <鎻忚堪>`
- `/mcp__rustplc__two_cylinder_template <start_mode> <cycle_mode>`
- `/mcp__rustplc__extern_function_template <鍙傛暟>`
- `/mcp__rustplc__pid_control_template <鍙傛暟>`

## 鏁呴殰鎺掓煡

### 闂锛歁CP 鏈嶅姟鍣ㄦ棤娉曞惎鍔?
**瑙ｅ喅鏂规锛?*
1. 纭宸插畨瑁?`mcp` 鍖咃細`pip install mcp`
2. 纭 Python 鐗堟湰 >= 3.10
3. 妫€鏌?`.mcp.json` 涓殑璺緞鏄惁姝ｇ‘

### 闂锛歷alidate_plc 鎶ラ敊 "rustplc binary not found"

**瑙ｅ喅鏂规锛?*
1. 纭宸叉瀯寤虹紪璇戝櫒锛歚cargo build --release`
2. 妫€鏌?`.mcp.json` 涓殑 `RUSTPLC_PATH` 鏄惁姝ｇ‘
3. 鎴栨墜鍔ㄨ缃幆澧冨彉閲忥細`export RUSTPLC_PATH=/path/to/rust_plc.exe`

### 闂锛氭棤娉曡闂ず渚嬫枃浠?
**瑙ｅ喅鏂规锛?*
1. 纭 `examples/` 鐩綍瀛樺湪浜庨」鐩牴鐩綍
2. 纭 `.mcp.json` 涓殑 `cwd` 璺緞姝ｇ‘

## 涓嬩竴姝?
- 鏌ョ湅 [README.md](README.md) 浜嗚В瀹屾暣鍔熻兘
- 鏌ョ湅 [docs/mcp_server_design.md](../docs/mcp_server_design.md) 浜嗚В鏋舵瀯璁捐
- 灏濊瘯鐢熸垚浣犵殑绗竴涓?PLC 绋嬪簭锛?
## 鍙嶉涓庤础鐚?
閬囧埌闂鎴栨湁寤鸿锛熸杩庡湪 GitHub 鎻?Issue 鎴?PR銆?
