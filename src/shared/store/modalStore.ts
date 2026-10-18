import { create } from "zustand";
import { ReactNode } from "react";

interface ModalState {
  isOpen: boolean;
  content: ReactNode | null;
  stack: ReactNode[];
  openModal: (content: ReactNode) => void;
  goBack: () => void;
  closeModal: () => void;
}

export const useModalStore = create<ModalState>((set, get) => ({
  isOpen: false,
  content: null,
  stack: [],
  openModal: (content) => {
    const { isOpen, content: currentContent, stack } = get();
    if (isOpen && currentContent) {
      set({ stack: [...stack, currentContent], content });
    } else {
      set({ isOpen: true, content, stack: [] });
    }
  },
  goBack: () => {
    const { stack } = get();
    if (stack.length === 0) return;
    const newStack = [...stack];
    const previousContent = newStack.pop()!;
    set({ content: previousContent, stack: newStack });
  },
  closeModal: () => set({ isOpen: false, content: null, stack: [] }),
}));
