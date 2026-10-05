<script setup lang="ts"></script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="title">
        <h1>SI BBS</h1>
        <span class="sub">AI 主题社区：Wiki 知识库 + 软件发布及索引</span>
      </div>
    </div>

    <div class="section">
      <div class="section-title">开始</div>
      <p class="section-hint">选择一个入口</p>
      <div class="list">
        <RouterLink class="list-row" to="/wiki">
          <div class="row-main">
            <span class="row-title">浏览 Wiki</span>
            <span class="row-sub">沉淀的技术文档与操作指南</span>
          </div>
        </RouterLink>
        <RouterLink class="list-row" to="/projects">
          <div class="row-main">
            <span class="row-title">发现项目</span>
            <span class="row-sub">提交并索引 GitHub 上的软件</span>
          </div>
        </RouterLink>
        <RouterLink class="list-row" to="/projects/submit">
          <div class="row-main">
            <span class="row-title">提交项目</span>
            <span class="row-sub">分享你的作品给社区</span>
          </div>
        </RouterLink>
      </div>
    </div>
  </div>
</template>
