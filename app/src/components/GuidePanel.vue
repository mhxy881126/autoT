<script setup lang="ts">
const emit = defineEmits<{ (e: "close"): void }>();
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="modal">
      <div class="head">
        <b>怎么用这套系统做 T</b>
        <button class="x" @click="emit('close')">×</button>
      </div>

      <ol class="steps">
        <li>
          <b>先配置账户。</b>点右上角「设置」：标的代码、底仓数量、底仓成本、可用资金、
          单次 T 仓上限。底仓是前提——没有底仓，A 股 T+1 下做不了 T。
        </li>
        <li>
          <b>看环境，不急着找买点。</b>regime 为「震荡」才考虑做 T；上行禁止倒 T（防卖飞），
          下行禁止正 T（防越买越套），极端行情全禁。
        </li>
        <li>
          <b>读决策卡片。</b>「通过」= 按卡片参数执行；「拒绝交易」= 读下方否决理由
          （空间不足 / 扣成本无优势 / regime 不允许）；「等待信号」= 今天没机会，不动。
        </li>
        <li>
          <b>在券商 App 手工下单。</b>系统只给参数，不接券商、不下单：
          正 T 先买后卖，倒 T 先卖后买。价格、数量、目标、止损都按卡片来。
        </li>
        <li>
          <b>成交后回填或导入交割单。</b>账务页可以逐笔回填，也可以导入券商 CSV。
          不回填，账本和复盘全是假的。
        </li>
        <li>
          <b>收盘复盘。</b>看配对（正 T / 倒 T）、已实现盈亏、未闭环笔数。
          未闭环的 T 次日开盘必须处理，不能装作没发生。
        </li>
      </ol>

      <div class="box">
        <div><span>正 T</span> 先买后卖，用底仓的可卖额度完成闭环，赚价差、降成本。</div>
        <div><span>倒 T</span> 先卖后买，先卖底仓再低位买回，适合冲高回落。</div>
        <div><span>成本</span> 一次往返约 0.142%（佣金 + 印花税 + 过户费 + 滑点），价差太小的 T 不做。</div>
      </div>

      <div class="warn">
        做 T 不是稳赚。多数交易日、多数股票，正确答案是「不做」。
        系统的价值是帮你算清成本、拦住烂交易，而不是催你下单。
      </div>

      <div class="actions">
        <button class="primary" @click="emit('close')">知道了</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  z-index: 1000;
  display: grid;
  place-items: center;
}
.modal {
  width: 640px;
  max-height: 86vh;
  overflow: auto;
  background: #111620;
  border: 1px solid #28313f;
  border-radius: 10px;
  padding: 18px;
  color: #dbe2ee;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
  font-size: 15px;
}
.x {
  background: none;
  border: 0;
  color: #76839a;
  font-size: 20px;
  cursor: pointer;
}
.steps {
  margin: 0;
  padding-left: 18px;
  font-size: 12.5px;
  line-height: 1.9;
  color: #c3cddd;
}
.steps li {
  margin-bottom: 8px;
}
.steps b {
  color: #e8b339;
  font-weight: 600;
}
.box {
  margin-top: 12px;
  padding: 10px 12px;
  border: 1px solid #28313f;
  border-radius: 6px;
  background: #0e131c;
  font-size: 11.5px;
  line-height: 1.9;
  color: #9aa7bb;
}
.box span {
  display: inline-block;
  min-width: 42px;
  color: #dbe2ee;
  font-weight: 600;
}
.warn {
  margin-top: 12px;
  font-size: 11px;
  line-height: 1.7;
  color: #c98d16;
}
.actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}
.primary {
  background: #e8b339;
  border: 1px solid #e8b339;
  color: #1a1206;
  border-radius: 5px;
  padding: 7px 18px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
</style>
