// A tiny task queue, used as a golden highlighting sample.
package sample.queue

import kotlin.math.max

/** A unit of work with a priority. */
data class Task(val id: Int, val name: String, val priority: Int = 0)

sealed interface Outcome {
    object Done : Outcome
    data class Failed(val reason: String) : Outcome
}

enum class State { IDLE, RUNNING, STOPPED }

annotation class Experimental

class TaskQueue(private val capacity: Int) {
    private val tasks = mutableListOf<Task>()
    var state: State = State.IDLE
        private set

    companion object {
        const val DEFAULT_CAPACITY = 16
        fun withDefaults() = TaskQueue(DEFAULT_CAPACITY)
    }

    fun push(task: Task): Boolean {
        if (tasks.size >= capacity) return false
        tasks += task
        tasks.sortByDescending { it.priority }
        return true
    }

    @Experimental
    fun drain(handler: (Task) -> Outcome): Map<Int, Outcome> {
        state = State.RUNNING
        val results = mutableMapOf<Int, Outcome>()
        while (tasks.isNotEmpty()) {
            val next = tasks.removeAt(0)
            results[next.id] = try {
                handler(next)
            } catch (e: IllegalStateException) {
                Outcome.Failed(e.message ?: "unknown")
            }
        }
        state = State.STOPPED
        return results
    }
}

fun describe(outcome: Outcome): String = when (outcome) {
    is Outcome.Done -> "done"
    is Outcome.Failed -> "failed: ${outcome.reason}"
}

fun main() {
    val queue = TaskQueue.withDefaults()
    for (i in 1..3) {
        queue.push(Task(i, "task-$i", priority = max(i, 2)))
    }
    val ratio = 0.75f
    val mask = 0xFF
    val letter = 'q'
    val outcomes = queue.drain { task ->
        if (task.id % 2 == 0) Outcome.Done else Outcome.Failed("odd id")
    }
    outcomes.forEach { (id, outcome) -> println("$id ${describe(outcome)}") }
    println(listOf(ratio, mask, letter, null, true))
}
