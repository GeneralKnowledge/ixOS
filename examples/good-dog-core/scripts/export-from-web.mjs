// Exports Good Dog's content and records the TypeScript engine's output for
// the Rust parity tests. Run through export-from-web.sh, which prepares `dir`.
import fs from "node:fs";

const [dir, out] = process.argv.slice(2);
const { EXERCISE_LIBRARY, TOPIC_GROUPS } = await import(`${dir}/dog-training/content/exercises.ts`);
const { GLOSSARY, GLOSSARY_CATEGORIES } = await import(`${dir}/dog-training/content/glossary.ts`);
const { LEARNING_OBJECTIVES } = await import(`${dir}/dog-training/content/learning-objectives.ts`);
const { buildGoldenScenarios, GOLDEN_NOW } = await import(`${dir}/dog-training/golden-scenarios.ts`);
const { dogTrainingPolicy } = await import(`${dir}/dog-training/policy.ts`);
const { generateDailyPlan, pickPlanItemReplacement, applyOutcomeToProgress, DEFAULT_PROGRESSION_CONFIG } =
  await import(`${dir}/coaching/index.ts`);

fs.writeFileSync(`${out}/content/exercises.json`, `${JSON.stringify(EXERCISE_LIBRARY, null, 1)}\n`);
fs.writeFileSync(`${out}/content/learning-objectives.json`, `${JSON.stringify(LEARNING_OBJECTIVES, null, 1)}\n`);
fs.writeFileSync(`${out}/content/topics.json`, `${JSON.stringify(TOPIC_GROUPS, null, 1)}\n`);
// Published terms only, without the editorial fields the web app never shows.
const glossary = GLOSSARY.filter((t) => t.published).map(({ editorialNotes, needsQualifiedReview, published, ...term }) => term);
fs.writeFileSync(`${out}/content/glossary.json`, `${JSON.stringify({ categories: GLOSSARY_CATEGORIES, terms: glossary }, null, 1)}\n`);

// FNV-1a 64 over UTF-8, as in tests/common/mod.rs.
function fnv(text) {
  let h = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(text)) {
    h ^= BigInt(byte);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

// One day's plan plus the swap for its first item, in the form the Rust tests rebuild.
function planDay(input) {
  const plan = generateDailyPlan(input);
  let swap = null;
  if (plan.items.length > 0) {
    swap = pickPlanItemReplacement({
      ...input,
      excludeExerciseIds: plan.items.map((i) => i.exerciseId),
      replaceExerciseId: plan.items[0].exerciseId,
      itemRole: plan.items[0].role,
    });
  }
  const ids = `${plan.items.map((i) => i.exerciseId).join(",")}>${swap ? swap.exerciseId : "-"}`;
  const text = [
    ...plan.items.map((i) => `${i.exerciseId} | ${i.role} | ${i.whyToday}`),
    `${plan.meta.shortPlan}/${plan.meta.totalMinutes}/${plan.meta.reason}`,
    swap ? `${swap.exerciseId} | ${swap.whyToday}` : "-",
  ].join("\n");
  return { plan, ids, text };
}

const recorded = { scenarios: {}, weeks: {} };

// Every golden scenario (the web fixture samples one in nine).
for (const s of buildGoldenScenarios()) {
  const { ids, text } = planDay({
    subject: s.dog, policy: dogTrainingPolicy, exercises: EXERCISE_LIBRARY,
    progressByObjective: s.progress, recentSessions: s.recent, shortPlan: s.shortPlan, now: GOLDEN_NOW,
  });
  recorded.scenarios[s.key] = { ids, digest: fnv(text) };
}

// A week of planning and practice per dog, feeding outcomes back into
// progress: covers recent-repeat avoidance, welfare cooldowns, the calm list
// and the progression state machine, which the static scenarios do not.
const DAY = 86_400_000;
const STATES = ["not_introduced", "introduced", "practising", "becoming_consistent", "ready_to_increase", "needs_easier"];
const OUTCOMES = ["easy", "easy", "getting_there", "too_difficult"];
const byId = new Map(EXERCISE_LIBRARY.map((e) => [e.id, e]));
for (const lifeStage of ["young_puppy", "older_puppy", "adolescent", "adult", "senior"])
  for (const availableTime of ["few_minutes", "about_10", "more"])
    for (const primaryReason of ["", "Recall help", "Settle at home"])
      for (let start = -1; start < STATES.length; start++) {
        const dog = { id: "d", name: "Moss", lifeStage, availableTime, primaryReason, trainingExperience: start < 0 ? "new" : "some" };
        let progress = {};
        if (start >= 0) {
          LEARNING_OBJECTIVES.forEach((o, i) => {
            const state = STATES[(i + start) % STATES.length];
            progress[o.id] = { state, easyStreak: state === "needs_easier" || state === "not_introduced" ? 0 : (i % 4) + 1, recentOutcomes: [] };
          });
        }
        const recent = [];
        const days = [];
        const texts = [];
        let k = 0;
        for (let d = 0; d < 7; d++) {
          const now = GOLDEN_NOW + d * DAY;
          const { plan, ids, text } = planDay({
            subject: dog, policy: dogTrainingPolicy, exercises: EXERCISE_LIBRARY,
            progressByObjective: progress, recentSessions: recent, shortPlan: d === 3, now,
          });
          days.push(ids);
          texts.push(text);
          plan.items.forEach((item, j) => {
            const ex = byId.get(item.exerciseId);
            const outcome = OUTCOMES[k % 4];
            const welfareConcern = k % 7 === 5;
            const completedAt = now + 600_000 + j * 60_000;
            k++;
            recent.unshift({ exerciseId: ex.id, learningObjectiveId: ex.learningObjectiveId, completedAt, outcome, welfareConcern });
            const current = progress[ex.learningObjectiveId] ?? { state: "not_introduced", easyStreak: 0, recentOutcomes: [] };
            progress = { ...progress, [ex.learningObjectiveId]: applyOutcomeToProgress(current, outcome, welfareConcern, DEFAULT_PROGRESSION_CONFIG, completedAt) };
          });
        }
        for (const key of Object.keys(progress).sort()) {
          texts.push(`${key}=${progress[key].state}/${progress[key].easyStreak}`);
        }
        recorded.weeks[`${lifeStage}|${availableTime}|${primaryReason}|start=${start}`] = { days, digest: fnv(texts.join("\n")) };
      }

const lines = (o) => Object.entries(o).map(([k, v]) => `  ${JSON.stringify(k)}: ${JSON.stringify(v)}`).join(",\n");
fs.writeFileSync(`${out}/tests/fixtures/recorded.json`,
  `{\n "scenarios": {\n${lines(recorded.scenarios)}\n },\n "weeks": {\n${lines(recorded.weeks)}\n }\n}\n`);
console.log(`${EXERCISE_LIBRARY.length} exercises, ${Object.keys(recorded.scenarios).length} scenarios, ${Object.keys(recorded.weeks).length} weeks`);
