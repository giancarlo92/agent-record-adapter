// Part of Agent Record Adapter (see NOTICE).
// A small Java Swing window with a text field and an Apply button, to check keys, typed text and a
// click in an app that offers no accessibility tree (Swing exposes none to UI Automation unless the
// Java Access Bridge is turned on). Its title shows the field's text, and "applied <text>" after the
// button, so a test reads it from the window list. The button sits at a fixed place of the window
// (10, 60 inside the content, 120 x 30), for a click at a point.
// Run: java swing-text.java
import javax.swing.*;
import javax.swing.event.DocumentEvent;
import javax.swing.event.DocumentListener;

public class SwingText {
    public static void main(String[] args) {
        SwingUtilities.invokeLater(() -> {
            JFrame frame = new JFrame("Swing text test: ");
            frame.setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);
            JPanel panel = new JPanel(null);
            JTextField field = new JTextField();
            field.setBounds(10, 15, 380, 28);
            JButton apply = new JButton("Apply");
            apply.setBounds(10, 60, 120, 30);
            field.getDocument().addDocumentListener(new DocumentListener() {
                void show() { frame.setTitle("Swing text test: " + field.getText()); }
                public void insertUpdate(DocumentEvent e) { show(); }
                public void removeUpdate(DocumentEvent e) { show(); }
                public void changedUpdate(DocumentEvent e) { show(); }
            });
            apply.addActionListener(e -> frame.setTitle("Swing text test: applied " + field.getText()));
            panel.add(field);
            panel.add(apply);
            frame.setContentPane(panel);
            frame.setSize(420, 160);
            frame.setVisible(true);
            field.requestFocusInWindow();
        });
    }
}
