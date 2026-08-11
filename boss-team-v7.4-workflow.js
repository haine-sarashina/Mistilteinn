export const meta = {
  name: 'boss-team-v7.4',
  description: 'Boss Team v7.4 workflow for Mistilteinn browser engine - focused on amazon.co.jp rendering target',
  phases: [
    { title: 'Analysis', detail: 'Analyze current project state and amazon.co.jp rendering requirements' },
    { title: 'Planning', detail: 'Design implementation plan for amazon.co.jp rendering improvements' },
    { title: 'Implementation', detail: 'Execute code changes in parallel focusing on key modules' },
    { title: 'Testing', detail: 'Validate functionality with amazon.co.jp rendering test cases' },
    { title: 'Optimization', detail: 'Refine and optimize for performance' },
    { title: 'Documentation', detail: 'Update documentation for new improvements' },
    { title: 'Review', detail: 'Session review and improvements' }
  ]
}

// タスクの定義 - amazon.co.jpレンダリングターゲットに焦点を当てる
const tasks = [
  { id: 'html-parser', name: 'HTML Parser Enhancement for amazon.co.jp', component: 'html' },
  { id: 'css-parser', name: 'CSS Parser Enhancement for amazon.co.jp', component: 'css' },
  { id: 'layout-engine', name: 'Layout Engine Optimization for amazon.co.jp', component: 'layout' },
  { id: 'render-pipeline', name: 'Render Pipeline Improvements for amazon.co.jp', component: 'render' }
]

// 現在のプロジェクト状態を分析
phase('Analysis')
const projectState = await agent('Analyze the current state of Mistilteinn browser engine. Focus on how well it renders amazon.co.jp homepage including HTML structure, CSS layout, images and interactive elements. Identify key areas for improvement.', {schema: {type: 'object', properties: {status: {type: 'string'}, components: {type: 'array', items: {type: 'string'}}, issues: {type: 'array', items: {type: 'string'}}, rendering_performance: {type: 'string'}}}})

phase('Planning')
const plans = await parallel(tasks.map(task => async () => {
  return await agent(`Create implementation plan for ${task.name} in ${task.component} module. Focus on making amazon.co.jp render correctly with attention to DOM nodes, styles, layout, render rects, images and interactive elements.`, {schema: {type: 'object', properties: {plan: {type: 'string'}, priority: {type: 'string'}, estimated_time: {type: 'string'}, target_improvement: {type: 'string'}}}})
}))

phase('Implementation')
const implementations = await parallel(tasks.map((task, index) => async () => {
  return await agent(`Implement ${task.name} in ${task.component} module. Use the plan: ${JSON.stringify(plans[index])}. Keep changes focused and testable. Ensure improvements align with amazon.co.jp rendering requirements.`, {schema: {type: 'object', properties: {changes: {type: 'string'}, files_modified: {type: 'array', items: {type: 'string'}}, test_cases: {type: 'array', items: {type: 'string'}}, target_alignment: {type: 'string'}}}})
}))

phase('Testing')
const verifications = await parallel(implementations.map((impl, index) => async () => {
  return await agent(`Verify the implementation of ${tasks[index].name}. Test if it correctly handles the amazon.co.jp rendering requirements including DOM nodes, styles, layout, render rects, images and interactive elements.`, {schema: {type: 'object', properties: {status: {type: 'string'}, issues_found: {type: 'array', items: {type: 'string'}}, test_results: {type: 'array', items: {type: 'string'}}, improvement_score: {type: 'number'}}}})
}))

phase('Optimization')
const optimizations = await parallel(implementations.map((impl, index) => async () => {
  return await agent(`Optimize ${tasks[index].name} implementation. Focus on performance improvements while maintaining correct amazon.co.jp rendering.`, {schema: {type: 'object', properties: {optimizations: {type: 'array', items: {type: 'string'}}, performance_improvement: {type: 'string'}, memory_usage: {type: 'string'}}}})
}))

phase('Documentation')
const documentation = await agent('Update project documentation to reflect the new improvements for amazon.co.jp rendering target. Focus on how these changes affect rendering performance, DOM handling, CSS parsing, layout and GPU rendering.', {schema: {type: 'object', properties: {updated_docs: {type: 'string'}, doc_changes: {type: 'array', items: {type: 'string'}}, rendering_benchmark: {type: 'string'}}}})

phase('Review')
const retrospective = await agent('Review the entire workflow process. Identify what worked well and what could be improved for future iterations. Focus on amazon.co.jp rendering improvements and overall project quality.', {schema: {type: 'object', properties: {process_evaluation: {type: 'string'}, improvements: {type: 'array', items: {type: 'string'}}, lessons_learned: {type: 'string'}, next_steps: {type: 'array', items: {type: 'string'}}}}})

return {
  projectState,
  plans,
  implementations,
  verifications,
  optimizations,
  documentation,
  retrospective
}