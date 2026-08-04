import { create } from "zustand";
import type { TaskInfo } from "../lib/types";

interface TasksState {
  tasks: TaskInfo[];
  setTasks: (tasks: TaskInfo[]) => void;
}

export const useTasksStore = create<TasksState>((set) => ({
  tasks: [],
  setTasks: (tasks) => set({ tasks }),
}));
