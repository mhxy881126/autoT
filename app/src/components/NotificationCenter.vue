<script setup lang="ts">
import { useDeskStore } from "../stores/desk";
import { call, isTauri } from "../lib/ipc";
import type { AlertItem } from "../lib/types";

const emit = defineEmits<{ (e: "close"): void }>();
const store = useDeskStore();

async function testAlert(): Promise<void> {
  if (isTauri()) {
    try {
      await call<void>("test_alert");
    } catch {
      // 事件失败时退回本地插入，至少能看到消息中心样式
      pushLocal();
    }
  } else {
    pushLocal();
  }
}

function pushLocal(): void {
  store.pushAlert({
    symbol: "test",
    name: "测试提醒",
    accepted: 1,
    side: "正T",
    t: "10:30",
    target: 0,
    at: new Date().toLocaleTimeString("zh-CN", { hour12: false }),
    test: true
  });
}

async function openAlert(item: AlertItem): Promise<void> {
  item.read = true;
  if (item.test || !item.symbol) return;
  try {
    await store.switchSymbol(item.symbol);
    emit("close");
  } catch {
    // 切换失败保留面板
  }
}
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <aside class="drawer">
      <div class="head">
        <b>消息</b>
        <span v-if="store.unread" class="unread">{{ store.unread }} 条未读</span>
        <span class="spacer"></span>
        <button class="x" @click="emit('close')">×</button>
      </div>

      <div class="tools">
        <button class="ghost" @click="testAlert">测试一条</button>
        <button class="ghost" @click="store.markAlertsRead()">全部已读</button>
        <button class="ghost" @click="store.clearAlerts()">清空</button>
      </div>

      <div v-if="!store.alerts.length" class="empty">
        还没有消息。<br />
        扫描到“通过”信号时会自动出现在这里，点一条可直接切到该票。
      </div>

      <div v-else class="list">
        <div
          v-for="item in store.alerts"
          :key="item.id"
          class="item"
          :class="{ unread: !item.read, test: item.test }"
          @click="openAlert(item)"
        >
          <div class="row">
            <span class="name">{{ item.name || item.symbol }}</span>
            <span class="at">{{ item.at }}</span>
          </div>
          <div class="row">
            <span class="sig">
              {{ item.test ? "测试" : `${item.accepted} 个信号` }}
              · {{ item.side }} {{ item.t }}
            </span>
            <span v-if="item.target" class="target">目标 {{ item.target.toFixed(2) }}</span>
          </div>
        </div>
      </div>

      <div class="foot">
        系统通知是兜底；应用内消息不会因为专注助手或权限被吞掉。
      </div>
    </aside>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  z-index: 900;
}
.drawer {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 360px;
  background: #111620;
  border-left: 1px solid #28313f;
  display: flex;
  flex-direction: column;
  color: #dbe2ee;
  box-shadow: -12px 0 40px rgba(0, 0, 0, 0.4);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 14px;
  border-bottom: 1px solid #1d2431;
  font-size: 14px;
}
.unread {
  font-size: 10px;
  color: #ef5350;
}
.spacer {
  flex: 1;
}
.x {
  background: none;
  border: 0;
  color: #76839a;
  font-size: 20px;
  cursor: pointer;
}
.tools {
  display: flex;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid #1d2431;
}
.ghost {
  background: #1a2230;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 10px;
  cursor: pointer;
}
.empty {
  padding: 28px 18px;
  font-size: 11px;
  line-height: 1.9;
  color: #4d5a70;
}
.list {
  flex: 1;
  overflow: auto;
}
.item {
  padding: 10px 14px;
  border-bottom: 1px solid #161d29;
  cursor: pointer;
  border-left: 2px solid transparent;
}
.item:hover {
  background: #151c27;
}
.item.unread {
  border-left-color: #ef5350;
  background: rgba(239, 83, 80, 0.04);
}
.item.test {
  opacity: 0.75;
}
.row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  gap: 8px;
}
.name {
  font-size: 12px;
  color: #e8eef8;
}
.at {
  font: 9px Consolas, monospace;
  color: #4d5a70;
}
.sig {
  font-size: 10px;
  color: #ef5350;
  margin-top: 3px;
}
.item.test .sig {
  color: #76839a;
}
.target {
  font: 9px Consolas, monospace;
  color: #76839a;
}
.foot {
  padding: 10px 14px;
  border-top: 1px solid #1d2431;
  font-size: 9px;
  line-height: 1.6;
  color: #4d5a70;
}
</style>
