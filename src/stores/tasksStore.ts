import { create } from "zustand";
import type { TaskInfo } from "../lib/types";

// Conteggio globale dei task della sessione corrente del tool: serve a decidere
// se mostrare la voce "Task" nella barra laterale (solo se ce n'è almeno uno).
interface TasksState {
  tasks: TaskInfo[];
  setTasks: (tasks: TaskInfo[]) => void;
}

export const useTasksStore = create<TasksState>((set) => ({
  tasks: [],
  setTasks: (tasks) => set({ tasks }),
}));
